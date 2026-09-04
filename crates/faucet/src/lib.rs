use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use anyhow::Context;
use miden_client::account::component::{
    AccessControl,
    BasicConstantFeePolicy,
    BasicWallet,
    BurnPolicy,
    FeePolicyManager,
    FungibleFaucet,
    MintPolicy,
    Ownable2Step,
    TokenName,
    TokenPolicyManager,
    TransferPolicy,
    create_network_fungible_faucet,
};
use miden_client::account::{
    Account,
    AccountBuilder,
    AccountComponent,
    AccountId,
    AccountReader,
    AccountType,
    Address,
    NetworkId,
    StorageSlotContent,
};
use miden_client::asset::{
    AssetAmount as ProtocolAssetAmount,
    AssetId,
    FungibleAsset,
    TokenSymbol,
};
use miden_client::auth::{Approver, AuthScheme, AuthSecretKey, AuthSingleSig};
use miden_client::block::{BlockNumber, FeeParameters};
use miden_client::builder::ClientBuilder;
use miden_client::crypto::RandomCoin;
use miden_client::crypto::rpo_falcon512::SecretKey;
use miden_client::keystore::{FilesystemKeyStore, Keystore};
use miden_client::note::standards::BurnNote;
use miden_client::note::{
    FeeSponsorshipNote,
    MintNote,
    MintNoteStorage,
    NetworkAccountTarget,
    Note,
    NoteDetails,
    NoteError,
    NoteExecutionHint,
    NoteId,
    NoteType as ProtocolNoteType,
    P2idNote,
    P2idNoteStorage,
    P2ideNote,
    P2ideNoteStorage,
};
use miden_client::rpc::domain::account::{AccountStorageRequirements, GetAccountRequest};
use miden_client::rpc::{Endpoint, GrpcClient, GrpcError, NodeRpcClient, RpcError};
use miden_client::store::{NoteFilter, TransactionFilter};
use miden_client::sync::{StateSync, StateSyncInput, SyncSummary};
use miden_client::transaction::{
    ForeignAccount,
    LocalTransactionProver,
    NoteArgs,
    TransactionId,
    TransactionProver,
    TransactionRequest,
    TransactionRequestBuilder,
    TransactionRequestError,
};
use miden_client::{Client, ClientError, Felt, RemoteTransactionProver, Word};
use miden_client_sqlite_store::SqliteStore;
use miden_tx::NetworkNotePricer;
use rand::{RngExt, rng};
use tokio::sync::mpsc::Receiver;
use tokio::sync::watch;
use tracing::{Instrument, error, info, info_span, instrument, warn};
use url::Url;

mod note_screener;
pub mod requests;
pub mod types;

use crate::note_screener::NoteScreener;
use crate::requests::{MintError, MintRequest, MintResponse, MintResponseSender};
use crate::types::AssetAmount;

const COMPONENT: &str = "miden-faucet-client";

const KEYSTORE_PATH: &str = "keystore";
/// How long a P2ID note is kept in the cache, in blocks, before it is pruned.
const NOTE_RETENTION_BLOCKS: u32 = 100;

/// How many blocks the transaction that sends the MINT note stays valid after its reference block.
const MINT_TX_EXPIRATION_DELTA: u16 = 10;

/// The operator is funded once its balance of the chain's fee asset falls below this many base
/// units.
const OPERATOR_FUNDING_THRESHOLD: u64 = 100_000_000;

/// How many base units of the faucet's asset a funding request mints to the operator.
const OPERATOR_FUNDING_AMOUNT: u64 = 900_000_000;

/// Blocks after which the operator may reclaim a sponsorship whose MINT note was never consumed.
/// Generous compared to `MINT_TX_EXPIRATION_DELTA`, since reclaiming a sponsorship of a note that
/// is still consumable would strand that note unpaid.
const SPONSORSHIP_RECLAIM_DELTA: u32 = 1_000;

const DEFAULT_ACCOUNT_ID_SETTING: &str = "faucet_default_account_id";
pub(crate) const DEFAULT_OPERATOR_ACCOUNT_ID_SETTING: &str = "faucet_operator_default_account_id";

// FAUCET CLIENT
// ================================================================================================

/// The faucet's account ID and network ID.
///
/// Used as a type safety mechanism to avoid confusion with user account IDs, and allows us to
/// implement traits.
#[derive(Clone)]
pub struct FaucetId {
    pub account_id: AccountId,
    pub network_id: NetworkId,
}

impl FaucetId {
    pub fn new(account_id: AccountId, network_id: NetworkId) -> Self {
        Self { account_id, network_id }
    }

    pub fn to_bech32(&self) -> String {
        Address::new(self.account_id).encode(self.network_id.clone())
    }
}

/// In-memory cache of the P2ID notes the network will mint from the faucet's MINT notes, keyed by
/// the hex note id.
///
/// The faucet's own transaction only creates MINT notes, so the resulting P2ID notes never land in
/// the client store. They are reconstructed at mint time and kept here so `get_note` can serve
/// them.
pub type P2idNoteCache = Arc<RwLock<HashMap<String, CachedP2idNote>>>;

/// A cached P2ID note together with a lower bound on where it can appear on chain.
#[derive(Clone)]
pub struct CachedP2idNote {
    /// The note the network will mint from the corresponding MINT note.
    pub note: Note,
    /// The chain tip when the MINT note was submitted.
    pub after_block_num: BlockNumber,
}

/// Stores the current faucet state and handles minting requests.
pub struct Faucet {
    id: FaucetId,
    client: Client<FilesystemKeyStore>,
    state_sync_component: StateSync,
    tx_prover: Arc<dyn TransactionProver>,
    issuance: watch::Sender<AssetAmount>,
    max_supply: AssetAmount,
    p2id_notes: P2idNoteCache,
    operator_account_id: AccountId,
    /// Whether there is an in-flight P2ID note to fund the operator account.
    funding_request_in_flight: bool,
}

/// Configuration for initializing and loading a faucet.
pub struct FaucetConfig {
    /// The path to the client store file.
    pub store_path: PathBuf,
    /// The endpoint of the node to connect to.
    pub node_endpoint: Endpoint,
    /// The network ID of the node to connect to.
    pub network_id: NetworkId,
    /// The timeout for the node connection.
    pub timeout: Duration,
    /// The remote prover url to use for proving transactions. If set to none, a local transaction
    /// prover is used.
    pub remote_tx_prover_url: Option<Url>,
}

/// The faucet account to initialize against.
pub enum FaucetAccount {
    /// A freshly created faucet account, to be added to the store as-is.
    New(Box<Account>),
    /// An existing on-chain faucet account, identified by its ID. Its state is fetched from the
    /// node.
    Existing(AccountId),
}

impl FaucetAccount {
    /// Returns the ID of the faucet account.
    pub fn id(&self) -> AccountId {
        match self {
            Self::New(account) => account.id(),
            Self::Existing(account_id) => *account_id,
        }
    }
}

impl Faucet {
    /// Initializes a new faucet client, creating the keystore and the database with the given
    /// accounts.
    ///
    /// A newly created faucet account ([`FaucetAccount::New`]) is deployed by submitting an empty
    /// transaction; an imported one ([`FaucetAccount::Existing`]) is already on-chain, so nothing
    /// is submitted.
    pub async fn init(
        config: &FaucetConfig,
        faucet_account: FaucetAccount,
        operator_secret_key: &AuthSecretKey,
        operator_account: Account,
    ) -> anyhow::Result<()> {
        let faucet_account_id = faucet_account.id();
        let deploy = matches!(faucet_account, FaucetAccount::New(_));

        let keystore =
            FilesystemKeyStore::new(KEYSTORE_PATH.into()).context("failed to create keystore")?;
        keystore.add_key(operator_secret_key, operator_account.id()).await?;

        let sqlite_store = Arc::new(SqliteStore::new(config.store_path.clone()).await?);

        let mut client = ClientBuilder::new()
            .grpc_client(&config.node_endpoint, Some(config.timeout.as_millis() as u64))
            .authenticator(Arc::new(keystore))
            .store(sqlite_store.clone())
            .build()
            .await?;

        client.ensure_genesis_in_place().await?;

        let fee_parameters = read_fee_parameters(&client).await?;
        if deploy {
            ensure_new_faucet_can_be_deployed(&fee_parameters)?;
        }

        // We sync to the chain tip before importing the account to avoid matching too many notes
        // tags from the genesis block (in case this is a fresh store).
        let note_screener = NoteScreener::new(sqlite_store);
        let grpc_client =
            Arc::new(GrpcClient::new(&config.node_endpoint, config.timeout.as_millis() as u64));
        let state_sync_component =
            StateSync::new(grpc_client.clone(), Arc::new(note_screener), None);

        // An imported faucet account is expected to be a deployed public account. Checking it here
        // reports a wrong account ID before anything is written to the store.
        if let FaucetAccount::Existing(account_id) = &faucet_account {
            let (_, account_proof) = grpc_client
                .get_account(*account_id, GetAccountRequest::new())
                .await
                .with_context(|| {
                    format!("failed to fetch faucet account {account_id} from the node")
                })?;
            anyhow::ensure!(
                account_proof.account_header().is_some(),
                "faucet account {account_id} has no public state on the node"
            );
        }

        Self::sync_state(&[operator_account.id()], &mut client, &state_sync_component).await?;

        let add_result = match &faucet_account {
            FaucetAccount::New(account) => client.add_account(account, false).await,
            // An existing faucet account is public, so its state is fetched from the node.
            FaucetAccount::Existing(account_id) => client.import_account_by_id(*account_id).await,
        };
        match add_result {
            Ok(()) => (),
            Err(ClientError::AccountAlreadyTracked(_)) => {
                warn!(
                    target: COMPONENT,
                    {
                        account.id = %faucet_account_id,
                        kind = "faucet"
                    },
                    "Faucet account already tracked, skipping import",
                );
            },
            Err(error) => {
                anyhow::bail!("failed to add account: {error}");
            },
        }
        // Check that the given operator is the actual owner of the faucet
        let faucet_account = client
            .get_account(faucet_account_id)
            .await
            .context("failed to read the faucet account from the store")?
            .with_context(|| format!("faucet account {faucet_account_id} is not tracked"))?;
        check_faucet_owner_matches_operator(&faucet_account, operator_account.id())?;

        client
            .set_setting(DEFAULT_ACCOUNT_ID_SETTING.to_owned(), faucet_account_id)
            .await?;

        let add_result = client.add_account(&operator_account, false).await;
        match add_result {
            Ok(()) => (),
            Err(ClientError::AccountAlreadyTracked(_)) => {
                warn!(
                    target: COMPONENT,
                    {
                        account.id = %operator_account.id(),
                        kind = "operator"
                    },
                    "Operator account already tracked, skipping import",
                );
            },
            Err(error) => anyhow::bail!("failed to add operator account: {error}"),
        }
        client
            .set_setting(DEFAULT_OPERATOR_ACCOUNT_ID_SETTING.to_owned(), operator_account.id())
            .await?;

        // A newly created faucet account is deployed by its first transaction. An imported one is
        // already on-chain, so there is nothing to deploy.
        if deploy {
            Self::deploy_faucet_account(faucet_account_id, config).await?;
        }

        info!(
            target: COMPONENT,
            {
                faucet.account.id = %faucet_account_id,
                operator.account.id = %operator_account.id(),
                faucet_account.status = if deploy { "created" } else { "imported" },
                store.path = %config.store_path.display(),
                node.endpoint = %config.node_endpoint
            },
            "Faucet initialized",
        );

        Ok(())
    }

    /// Loads the faucet with the given config.
    ///
    /// The account used is the default account set in the store, that is set on `Faucet::init`.
    #[instrument(
        target = COMPONENT,
        name = "faucet.load",
        fields(account_id, operator_account_id),
        skip_all,
        err
    )]
    pub async fn load(config: &FaucetConfig) -> anyhow::Result<Self> {
        let span = tracing::Span::current();
        let sqlite_store = Arc::new(SqliteStore::new(config.store_path.clone()).await?);
        let mut client = ClientBuilder::new()
            .grpc_client(&config.node_endpoint, Some(config.timeout.as_millis() as u64))
            .filesystem_keystore(KEYSTORE_PATH)?
            .store(sqlite_store.clone())
            .build()
            .await
            .context("failed to build client")?;

        let account_id: AccountId = client
            .get_setting(DEFAULT_ACCOUNT_ID_SETTING.to_owned())
            .await?
            .context("no default account id found")?;
        span.record("account_id", account_id.to_hex());

        let operator_account_id: AccountId = client
            .get_setting(DEFAULT_OPERATOR_ACCOUNT_ID_SETTING.to_owned())
            .await?
            .context("no default operator account id found")?;
        span.record("operator_account_id", operator_account_id.to_hex());

        // Try to update the account state with the node.
        let _ = client.import_account_by_id(account_id).await.inspect(|_| {
            info!("Received faucet account state from the node");
        });

        let account = client.get_account(account_id).await?.context("no account found")?;

        let token_metadata = FungibleFaucet::try_from(account.storage())?;
        let tx_prover: Arc<dyn TransactionProver> = match config.remote_tx_prover_url.clone() {
            Some(url) => Arc::new(RemoteTransactionProver::new(url)),
            None => Arc::new(LocalTransactionProver::default()),
        };
        let id = FaucetId::new(account.id(), config.network_id.clone());
        let max_supply = AssetAmount::new(token_metadata.max_supply().as_u64())?;
        let issuance_value = Self::read_issuance_from_store(&client, account.id()).await?;
        let (issuance, _) = watch::channel(issuance_value);

        let note_screener = NoteScreener::new(sqlite_store.clone());
        let grpc_client =
            Arc::new(GrpcClient::new(&config.node_endpoint, config.timeout.as_millis() as u64));
        let state_sync_component = StateSync::new(grpc_client, Arc::new(note_screener), None);

        let fee_parameters = read_fee_parameters(&client).await?;
        if fee_parameters.verification_base_fee() != 0 {
            // Both accounts are public, so a sync picks up funding that happened on chain after
            // `init` (for example the operator being funded out of band).
            Self::sync_state(
                &[account.id(), operator_account_id],
                &mut client,
                &state_sync_component,
            )
            .await
            .context("failed to sync before checking the fee asset balances")?;
            let faucet_reader = client.account_reader(account.id());
            if fee_asset_balance(&faucet_reader, &fee_parameters).await? == 0 {
                warn!(
                    target: COMPONENT,
                    {
                        faucet.account.id = %account.id(),
                        fee.faucet.id = %fee_parameters.fee_faucet_id(),
                    },
                    "The faucet account holds none of the chain's fee asset. The network \
                     transactions that consume its MINT notes pay their fee from the faucet's own \
                     vault, so they will fail until the faucet account is funded",
                );
            }
        }

        Ok(Self {
            id,
            client,
            state_sync_component,
            tx_prover,
            issuance,
            max_supply,
            p2id_notes: P2idNoteCache::default(),
            funding_request_in_flight: false,
            operator_account_id,
        })
    }

    /// Syncs the state of the client.
    #[instrument(target = COMPONENT, name = "faucet.sync_state", skip_all, err)]
    async fn sync_state(
        account_ids: &[AccountId],
        client: &mut Client<FilesystemKeyStore>,
        state_sync: &StateSync,
    ) -> anyhow::Result<SyncSummary> {
        let mut accounts = Vec::with_capacity(account_ids.len());
        for account_id in account_ids {
            match client.account_reader(*account_id).header().await {
                Ok((header, _)) => accounts.push(header),
                Err(error) => warn!(
                    target: COMPONENT,
                    {
                        account.id=%account_id,
                        %error
                    },
                    "Account is not tracked locally, excluding it from the sync",
                ),
            }
        }
        let output_notes = client.get_output_notes(NoteFilter::Expected).await?;
        let uncommitted_transactions =
            client.get_transactions(TransactionFilter::Uncommitted).await?;

        // The node matches note inclusions by tag, and skips the query altogether when no tag is
        // given. `add_account` registers a tag per tracked account, so passing them makes the sync
        // return the P2ID notes payable to the operator.
        let note_tags =
            client.get_note_tags().await?.into_iter().map(|record| record.tag).collect();
        // Tracked unspent notes must be followed too, so the funding notes the faucet consumes are
        // moved out of the committed state once their nullifiers show up on chain.
        let input_notes = client.get_input_notes(NoteFilter::Unspent).await?;

        // Build current partial MMR
        let mut current_partial_mmr = client.get_current_partial_mmr().await?;

        // Get the sync update from the network
        let state_sync_update = state_sync
            .sync_state(
                &mut current_partial_mmr,
                StateSyncInput {
                    accounts,
                    note_tags,
                    input_notes,
                    output_notes,
                    uncommitted_transactions,
                },
            )
            .await
            .context("failed to sync state")?;
        let sync_summary: SyncSummary = (&state_sync_update).into();

        // Apply received and computed updates to the store
        client
            .apply_state_sync(state_sync_update)
            .await
            .context("failed to apply state sync")?;

        Ok(sync_summary)
    }

    /// Runs the faucet minting process until the request source is closed, or it encounters a fatal
    /// error.
    ///
    /// It receives new minting requests and handles them in batches. For each request, it builds a
    /// minting note and updates the issuance counter. A transaction is created and submitted with
    /// all the notes from the batch. A `MintResponse` is sent through each response sender with the
    /// new note id and transaction id.
    ///
    /// Once the available supply is exceeded, any requests that exceed the supply will return an
    /// error. The request stream is closed and the minter shuts down.
    pub async fn run(
        &mut self,
        mut requests: Receiver<(MintRequest, MintResponseSender)>,
        batch_size: usize,
    ) -> anyhow::Result<()> {
        let mut buffer = Vec::new();

        while requests.recv_many(&mut buffer, batch_size).await > 0 {
            match Box::pin(self.mint(buffer.drain(..))).await {
                Ok(()) => (),
                Err(error) => {
                    if let Some(ClientError::RpcError(_)) = error.downcast_ref::<ClientError>() {
                        let error_chain = format!("{error:#}");
                        error!(error = %error_chain, "RPC error, discarding batch");
                    } else {
                        anyhow::bail!(error.context("failed to mint batch"));
                    }
                },
            }
        }
        info!(target = COMPONENT, "Request stream closed, shutting down minter");

        Ok(())
    }

    /// Mints a batch of requests.
    ///
    /// The requests size is guaranteed to be smaller or equal to the batch size set in
    /// `Faucet::run`.
    #[instrument(parent = None, target = COMPONENT, name = "faucet.mint", skip_all, fields(num_requests, tx_id), err(Debug))]
    async fn mint(
        &mut self,
        requests: impl IntoIterator<Item = (MintRequest, MintResponseSender)>,
    ) -> anyhow::Result<()> {
        // We sync before creating the transaction to ensure the state is up to date. If the
        // previous transaction somehow failed to be included in the block, our state would
        // be out of sync.
        // Both accounts need syncing, for different reasons: the operator executes the transaction,
        // and the faucet holds the token supply that `refresh_issuance` reads.
        let sync_summary = Self::sync_state(
            &[self.id.account_id, self.operator_account_id],
            &mut self.client,
            &self.state_sync_component,
        )
        .await?;
        let after_block_num = sync_summary.block_num;

        let span = tracing::Span::current();

        let (valid_requests, response_senders) = self.filter_requests_by_supply(requests);
        span.record("num_requests", valid_requests.len());

        if valid_requests.is_empty() {
            return Ok(());
        }

        let faucet_account = self.faucet_account().await.map_err(|error| *error)?;

        // Build notes
        let mut rng = {
            let auth_seed: [u64; 4] = rng().random();
            let rng_seed = Word::new(auth_seed.map(Felt::new_unchecked));
            RandomCoin::new(rng_seed)
        };
        // Build the P2ID notes first, the MINT notes are
        // derived from them below.
        let mut p2id_notes = build_p2id_notes(&self.faucet_id(), &valid_requests, &mut rng)?;
        let p2id_note_ids: Vec<NoteId> = p2id_notes.iter().map(Note::id).collect();

        // The operator pays for the transactions it submits, so the faucet funds it with its own
        // asset, minting one more P2ID note payable to it alongside the batch's.
        if self.operator_requires_funding().await? {
            p2id_notes.push(self.create_p2id_note_to_operator(&mut rng)?);
            self.funding_request_in_flight = true;
        }

        let mint_notes = build_mint_notes(
            self.faucet_id().account_id,
            &p2id_notes,
            &mut rng,
            self.operator_account_id,
        )?;

        // Log the P2id note ids along with their corresponding MINT note ids.
        for ((request, mint_note), p2id_note) in
            valid_requests.iter().zip(&mint_notes).zip(&p2id_notes)
        {
            info!(
                target: COMPONENT,
                {
                    mint_note.id = %mint_note.id().to_hex(),
                    p2id_note.id = %p2id_note.id().to_hex(),
                    target_account.id = %request.account_id,
                    note.type = ?request.note_type
                },
                "Built mint request",
            );
        }

        // Check whether there are any P2ID notes that fund the operator
        let operator_funding_notes = self.get_notes_targeted_to_operator(after_block_num).await?;
        if !operator_funding_notes.is_empty() {
            self.funding_request_in_flight = false;
        }

        // Build and submit transaction
        let fee_parameters = read_fee_parameters(&self.client).await?;
        let mut notes = mint_notes.clone();
        notes.extend(build_sponsorship_notes(
            self.operator_account_id,
            &faucet_account,
            &mint_notes,
            &fee_parameters,
            after_block_num + SPONSORSHIP_RECLAIM_DELTA,
            &mut rng,
        )?);

        let tx_request = Faucet::create_transaction(
            &notes,
            &operator_funding_notes,
            faucet_foreign_account(&faucet_account)?,
        )
        .context("faucet failed to create transaction")?;
        // The MINT notes are sent by the operator, so the operator must be the executing account.
        let tx_id = Box::pin(self.submit_new_transaction(self.operator_account_id, tx_request))
            .await
            .map_err(|error| *error)
            .context("faucet failed to submit transaction")?;
        span.record("tx_id", tx_id.to_string());
        info!(
            target: COMPONENT,
            {
                request_tx.id = %tx_id.to_hex(),
                mint_notes.num = mint_notes.len(),
                sponsorship_notes.num = notes.len() - mint_notes.len(),
                after_block_num = %after_block_num
            },
            "Submitted MINT notes; the network mints the P2ID notes in a later transaction",
        );

        // The faucet's transaction only creates the MINT notes; the P2ID notes are minted later by
        // the network, so they never land in the client store.
        // They are cached here for `get_note` to serve.
        {
            let mut cache = self.p2id_notes.write().expect("p2id note cache is poisoned");
            prune_stale_p2id_notes(&mut cache, after_block_num);
            // Only private notes are cached
            let private_notes = p2id_notes
                .into_iter()
                .filter(|note| matches!(note.metadata().note_type(), ProtocolNoteType::Private));
            for note in private_notes {
                cache.insert(note.id().to_hex(), CachedP2idNote { note, after_block_num });
            }
        }
        // Refresh the issuance cache from the store after submitting the transaction
        self.refresh_issuance().await;

        Self::send_responses(response_senders, p2id_note_ids, tx_id);
        Ok(())
    }

    /// Sends a `MintResponse` with the transaction id and note id through each of the response
    /// senders. Any errors while sending the response are ignored.
    #[instrument(target = COMPONENT, name = "faucet.mint.send_responses", skip_all)]
    fn send_responses(
        response_senders: Vec<MintResponseSender>,
        note_ids: Vec<NoteId>,
        tx_id: TransactionId,
    ) {
        for (sender, note_id) in response_senders.into_iter().zip(note_ids) {
            // Ignore errors if the request was dropped.
            let _ = sender.send(Ok(MintResponse { tx_id, note_id }));
        }
    }

    /// Updates the issuance counter for the requested amounts and filters the requests that exceed
    /// the available supply. For the filtered requests, the response sender is notified with an
    /// error.
    ///
    /// Returns a tuple of valid requests and response senders.
    #[instrument(target = COMPONENT, name = "faucet.mint.filter_requests_by_supply", skip_all)]
    fn filter_requests_by_supply(
        &self,
        requests: impl IntoIterator<Item = (MintRequest, MintResponseSender)>,
    ) -> (Vec<MintRequest>, Vec<MintResponseSender>) {
        let mut valid_requests = vec![];
        let mut response_senders = vec![];
        let mut issuance = *self.issuance.borrow();
        for (request, response_sender) in requests {
            let requested_amount = request.asset_amount;
            let available_amount = self.available_supply(issuance).unwrap_or_default();
            if available_amount < requested_amount {
                error!(
                    requested_amount = requested_amount.base_units(),
                    available_amount = available_amount.base_units(),
                    account_id = %request.account_id,
                    "Requested amount exceeds available supply",
                );
                let _ = response_sender.send(Err(MintError::AvailableSupplyExceeded));
                continue;
            }
            valid_requests.push(request);
            response_senders.push(response_sender);
            // SAFETY: creating an asset amount with the max is always valid
            issuance = issuance.checked_add(requested_amount).unwrap_or(AssetAmount::max());
        }
        if self.available_supply(issuance).is_none() {
            error!("Faucet has run out of tokens");
        }
        (valid_requests, response_senders)
    }

    /// Creates the operator's transaction that generates the given mint notes and consumes
    /// `input_notes`, the P2ID notes that fund the operator.
    ///
    /// The MINT notes target the faucet, a network account, so creating them prices each note
    /// through an FPI into the faucet's fee policy. `faucet_account` supplies the faucet as a
    /// foreign account for that call.
    #[instrument(target = COMPONENT, name = "faucet.mint.create_tx", skip_all, err)]
    fn create_transaction(
        notes: &[Note],
        input_notes: &[Note],
        faucet_account: ForeignAccount,
    ) -> Result<TransactionRequest, TransactionRequestError> {
        let notes: Vec<Note> = notes.to_vec();
        let input_notes: Vec<(Note, Option<NoteArgs>)> =
            input_notes.iter().map(|note| (note.clone(), None)).collect();
        TransactionRequestBuilder::new()
            .input_notes(input_notes)
            .own_output_notes(notes)
            .expiration_delta(MINT_TX_EXPIRATION_DELTA)
            .foreign_accounts([faucet_account])
            .build()
    }

    /// Executes, proves, and then submits a transaction using the local miden-client.
    /// This results in submitting the transaction to the node and updating the local db to track
    /// the created notes.
    #[instrument(
        target = COMPONENT,
        name = "faucet.mint.submit_new_transaction",
        skip_all,
        err,
        fields(
            account_id = %account_id,
            rpc.system = tracing::field::Empty,
            rpc.method = tracing::field::Empty,
            rpc.grpc.status_code = tracing::field::Empty,
            exception.type = tracing::field::Empty,
            exception.message = tracing::field::Empty,
        )
    )]
    async fn submit_new_transaction(
        &mut self,
        account_id: AccountId,
        tx_request: TransactionRequest,
    ) -> Result<TransactionId, Box<ClientError>> {
        // Execute the transaction
        let execute_span = info_span!(target: COMPONENT, "faucet.mint.execute", exception.message = tracing::field::Empty);
        let tx_result = self
            .client
            .execute_transaction(account_id, tx_request)
            .instrument(execute_span.clone())
            .await
            .inspect_err(|e| {
                execute_span.record("exception.message", tracing::field::display(e));
                record_grpc_error_fields(e);
            })?;
        let tx_id = tx_result.executed_transaction().id();

        let proven_transaction = {
            let remote_span = info_span!(
                target: COMPONENT,
                "faucet.mint.prove_remote",
                exception.message = tracing::field::Empty,
            );
            let remote_proven_transaction = self
                .client
                .prove_transaction_with(&tx_result, self.tx_prover.clone())
                .instrument(remote_span.clone())
                .await
                .inspect_err(|e| {
                    remote_span.record("exception.message", tracing::field::display(e));
                });
            match remote_proven_transaction {
                Ok(proven_transaction) => proven_transaction,
                Err(error) => {
                    error!(?error, "Failed to prove transaction with remote prover");
                    let local_span = info_span!(
                        target: COMPONENT,
                        "faucet.mint.prove_local",
                        exception.message = tracing::field::Empty,
                    );
                    self.client
                        .prove_transaction(&tx_result)
                        .instrument(local_span.clone())
                        .await
                        .inspect_err(|e| {
                        local_span.record("exception.message", tracing::field::display(e));
                        record_grpc_error_fields(e);
                    })?
                },
            }
        };

        let submit_span = info_span!(
            target: COMPONENT,
            "faucet.mint.submit_transaction",
            exception.message = tracing::field::Empty,
        );
        let submission_height = self
            .client
            .submit_proven_transaction(proven_transaction, &tx_result)
            .instrument(submit_span.clone())
            .await
            .inspect_err(|e| {
                submit_span.record("exception.message", tracing::field::display(e));
                record_grpc_error_fields(e);
            })?;

        let apply_span = info_span!(
            target: COMPONENT,
            "faucet.mint.apply_transaction",
            exception.message = tracing::field::Empty,
        );
        self.client
            .apply_transaction(&tx_result, submission_height)
            .instrument(apply_span.clone())
            .await
            .inspect_err(|e| {
                apply_span.record("exception.message", tracing::field::display(e));
                record_grpc_error_fields(e);
            })?;

        Ok(tx_id)
    }

    /// Returns the faucet account.
    pub async fn faucet_account(&self) -> Result<Account, Box<ClientError>> {
        self.client
            .get_account(self.id.account_id)
            .await?
            .ok_or_else(|| Box::new(ClientError::AccountDataNotFound(self.id.account_id)))
    }

    /// Returns the id of the faucet account.
    pub fn faucet_id(&self) -> FaucetId {
        self.id.clone()
    }

    /// Returns the available supply of the faucet.
    pub fn available_supply(&self, issuance: AssetAmount) -> Option<AssetAmount> {
        self.max_supply.checked_sub(issuance)
    }

    /// Returns a watch receiver that yields the current issuance value whenever it changes.
    /// The receiver immediately produces the latest value on subscription.
    pub fn subscribe_issuance(&self) -> watch::Receiver<AssetAmount> {
        self.issuance.subscribe()
    }

    /// Returns a handle to the cache of P2ID notes minted through this faucet's MINT notes.
    pub fn p2id_notes(&self) -> P2idNoteCache {
        self.p2id_notes.clone()
    }

    /// Returns the cached P2ID note, if it exists. Otherwise returns `None`.
    pub fn get_p2id_note(&self, note_id: NoteId) -> Option<CachedP2idNote> {
        self.p2id_notes
            .read()
            .expect("p2id note cache is poisoned")
            .get(&note_id.to_hex())
            .cloned()
    }

    /// Returns the id of the operator account that submits the MINT notes.
    pub fn operator_id(&self) -> AccountId {
        self.operator_account_id
    }

    /// Returns the chain's fee parameters, read from the latest block header the client has
    /// stored.
    pub async fn fee_parameters(&self) -> anyhow::Result<FeeParameters> {
        read_fee_parameters(&self.client).await
    }

    /// Reads the current issuance from the client's store and updates the watch channel.
    async fn refresh_issuance(&self) {
        let new_issuance = Self::read_issuance_from_store(&self.client, self.id.account_id)
            .await
            .unwrap_or(AssetAmount::max());
        self.issuance.send_replace(new_issuance);
    }

    /// Reads the current issuance from the client's store.
    async fn read_issuance_from_store(
        client: &Client<FilesystemKeyStore>,
        account_id: AccountId,
    ) -> anyhow::Result<AssetAmount> {
        let token_config_word = client
            .account_reader(account_id)
            .get_storage_item(FungibleFaucet::token_config_slot().clone())
            .await?;
        // The token config layout is `[token_supply, max_supply, decimals, token_symbol]`
        let token_supply = token_config_word[0].as_canonical_u64();
        Ok(AssetAmount::new(token_supply)?)
    }

    /// Deploys the faucet account by submitting its first transaction.
    async fn deploy_faucet_account(
        faucet_id: AccountId,
        config: &FaucetConfig,
    ) -> anyhow::Result<()> {
        let mut faucet = Self::load(config).await?;
        let empty_tx_request = TransactionRequestBuilder::new().build()?;
        let tx_id = Box::pin(faucet.submit_new_transaction(faucet_id, empty_tx_request)).await?;
        info!(
            target: COMPONENT,
            {
                account.id = %faucet_id,
                tx.id = %tx_id.to_hex()
            },
            "Deployed the faucet account",
        );
        Ok(())
    }

    /// Returns the P2ID notes that fund the operator.
    async fn get_notes_targeted_to_operator(
        &self,
        block_num: BlockNumber,
    ) -> anyhow::Result<Vec<Note>> {
        let operator_id = self.operator_id();
        let fee_faucet_id = self.fee_parameters().await?.fee_faucet_id();
        self.client
            .get_input_notes(NoteFilter::Committed)
            .await
            .context("failed to read the committed input notes from the store")?
            .iter()
            .filter(|record| {
                is_note_payable_to(record.details(), operator_id, fee_faucet_id)
                    && is_consumable_at(record.details(), block_num)
            })
            .map(|record| {
                record.try_into().context("failed to rebuild a P2ID note funding the operator")
            })
            .collect()
    }

    /// Whether the operator's balance of the chain's fee asset is below
    /// [`OPERATOR_FUNDING_THRESHOLD`] and no P2ID note funding the operator
    /// is already in flight.
    async fn operator_requires_funding(&self) -> anyhow::Result<bool> {
        if self.funding_request_in_flight {
            return Ok(false);
        }

        let fee_parameters = self.fee_parameters().await?;
        if fee_parameters.fee_faucet_id() != self.id.account_id {
            return Ok(false);
        }

        let operator = self.client.account_reader(self.operator_account_id);
        let balance = fee_asset_balance(&operator, &fee_parameters)
            .await
            .context("failed to read the operator's fee asset balance")?;
        Ok(balance < OPERATOR_FUNDING_THRESHOLD)
    }

    /// Builds the P2ID note that funds the operator with the faucet's own asset.
    fn create_p2id_note_to_operator(&self, rng: &mut RandomCoin) -> anyhow::Result<Note> {
        let faucet_id = self.id.account_id;
        let asset = FungibleAsset::new(faucet_id, OPERATOR_FUNDING_AMOUNT)
            .context("the operator funding amount is not a valid asset amount")?;

        Ok(P2idNote::builder()
            .sender(faucet_id)
            .target(self.operator_account_id)
            .asset(asset)
            // The faucet finds the note by syncing, which only rebuilds public notes.
            .note_type(ProtocolNoteType::Public)
            .generate_serial_number(rng)
            .build()
            .context("failed to build the P2ID note funding the operator")?
            .into())
    }
}

// FEE HELPERS
// ================================================================================================

/// Reads the chain's fee parameters from the latest block header the client has stored.
async fn read_fee_parameters(client: &Client<FilesystemKeyStore>) -> anyhow::Result<FeeParameters> {
    let latest_block = client
        .get_latest_block_header()
        .await
        .context("failed to read the latest block header from the store")?;
    Ok(latest_block.fee_parameters().clone())
}

/// Fails when the faucet cannot deploy a newly created faucet account on a chain charging
/// `fee_parameters`.
///
/// The faucet deploys a new account with an empty transaction, which the account's own auth
/// procedure has to pay for out of a vault that is still empty. A deployment could fund itself by
/// consuming a note carrying enough of the fee asset, but the faucet does not build one, so this
/// deployment fails on a fee-charging chain.
fn ensure_new_faucet_can_be_deployed(fee_parameters: &FeeParameters) -> anyhow::Result<()> {
    anyhow::ensure!(
        fee_parameters.verification_base_fee() == 0,
        "cannot create a new faucet account on a chain that charges transaction fees \
         (verification base fee {}): the faucet deploys it with an empty transaction, which has \
         to pay for itself out of the new account's empty vault. Import an existing faucet with \
         `--import` and `--faucet-account-id` instead",
        fee_parameters.verification_base_fee(),
    );
    Ok(())
}

/// Returns the amount of the chain's fee asset held in `account`'s vault, in base units.
///
/// The balance is read straight from the store, without loading the whole account.
pub async fn fee_asset_balance(
    account: &AccountReader,
    fee_parameters: &FeeParameters,
) -> anyhow::Result<u64> {
    let balance = account
        .get_balance(fee_parameters.fee_faucet_id())
        .await
        .context("failed to read the fee asset balance from the store")?;
    Ok(balance.as_u64())
}

/// Builds a `FEE_SPONSORSHIP` note for each MINT note, prepaying the network transaction that
/// consumes it so the faucet does not pay out of its own vault. The notes are public, so the node
/// discovers them in the committed block and bundles each with the MINT note it is bound to, and
/// their assets return to the operator through a reclaim if that MINT note is never consumed.
///
/// Returns no notes on a fee-free chain, or when the faucet does not collect fees in the chain's
/// native fee asset - the node drops such sponsorships before selection, so they would only strand
/// the operator's funds until their reclaim height. A faucet generated at genesis collects in its
/// operator's asset, which no asset can be issued by, until a release containing
/// <https://github.com/0xMiden/protocol/pull/3588> lets it collect in its own.
fn build_sponsorship_notes(
    operator_id: AccountId,
    faucet_account: &Account,
    mint_notes: &[Note],
    fee_parameters: &FeeParameters,
    reclaim_height: BlockNumber,
    rng: &mut RandomCoin,
) -> anyhow::Result<Vec<Note>> {
    let fee_asset_id = AssetId::new_fungible(fee_parameters.fee_faucet_id());
    let collected_asset_id = faucet_account
        .storage()
        .get_item(FeePolicyManager::fee_asset_id_slot())
        .context("failed to read the faucet's fee asset id")?;
    if fee_parameters.verification_base_fee() == 0 || collected_asset_id != fee_asset_id.to_word() {
        return Ok(Vec::new());
    }

    // The sponsorship is priced by the note's benchmarked consumption cost rather than by the
    // faucet's fee policy: the policy states what the faucet requires, while the epilogue
    // withdraws what the transaction actually costs. Over-payment is allowed - fee collection only
    // asserts that what a note owes is covered - and the excess stays in the faucet's vault.
    let amount = NetworkNotePricer::builder()
        .fee_parameters(fee_parameters.clone())
        .build()
        .price(MintNote::script_root())
        .context("failed to price the MINT note's consumption")?;
    let asset = FungibleAsset::new(fee_parameters.fee_faucet_id(), amount.as_u64())
        .context("failed to build the sponsorship's fee asset")?;

    mint_notes
        .iter()
        .map(|mint_note| {
            let note = FeeSponsorshipNote::builder()
                .sender(operator_id)
                .target_account(faucet_account.id())
                .feature_note_id(mint_note.id())
                .asset(asset)
                .reclaimer(operator_id)
                .reclaim_height(reclaim_height)
                .generate_serial_number(rng)
                .build()
                .context("failed to build a FEE_SPONSORSHIP note")?;
            Ok(note.into())
        })
        .collect()
}

/// Declares the faucet as a foreign account of the operator's MINT transaction.
///
/// Creating a note with a `NetworkAccountTarget` attachment prices the note through an FPI into
/// the target's fee policy, which reads the faucet's storage maps. Requesting every entry of every
/// map slot up front serves the whole FPI from a single RPC call anchored at the transaction's
/// reference block. It also keeps the request wire-compatible with node `0.16.0-rc.2`, which no
/// longer answers per-key storage map requests in the shape this client version decodes.
pub fn faucet_foreign_account(
    faucet_account: &Account,
) -> Result<ForeignAccount, TransactionRequestError> {
    let map_slots: Vec<_> = faucet_account
        .storage()
        .slots()
        .iter()
        .filter(|slot| matches!(slot.content(), StorageSlotContent::Map(_)))
        .map(|slot| slot.name().clone())
        .collect();
    ForeignAccount::public(faucet_account.id(), AccountStorageRequirements::all_entries(&map_slots))
}

// HELPER FUNCTIONS
// ================================================================================================

/// Records gRPC error details from a [`ClientError`] onto the current tracing span using the
/// OpenTelemetry RPC and exception semantic conventions
/// (<https://opentelemetry.io/docs/specs/semconv/rpc/grpc/>).
///
/// Sub-step errors propagate up to the parent `submit_new_transaction` span via `?`, but the
/// `#[instrument(..., err)]` macro only captures the error's `Display` output. This pulls the
/// structured fields out of [`RpcError::RequestError`] and records them as `rpc.system`,
/// `rpc.method`, `rpc.grpc.status_code`, `exception.type`, and `exception.message`.
fn record_grpc_error_fields(err: &ClientError) {
    let span = tracing::Span::current();
    if let ClientError::RpcError(rpc_err) = err {
        span.record("exception.message", tracing::field::display(rpc_err));
        if let RpcError::RequestError { endpoint, error_kind, endpoint_error, .. } = rpc_err {
            span.record("rpc.system", "grpc");
            span.record("rpc.method", tracing::field::display(endpoint));
            span.record("rpc.grpc.status_code", grpc_status_code(error_kind));
            span.record("exception.type", tracing::field::debug(error_kind));
            if let Some(ee) = endpoint_error {
                // Override with the more specific node-side message when available.
                span.record("exception.message", tracing::field::display(ee));
            }
        }
    }
}

/// Maps a [`GrpcError`] variant to its canonical gRPC numeric status code, as defined by
/// <https://github.com/grpc/grpc/blob/master/doc/statuscodes.md>. Used for the
/// `rpc.grpc.status_code` OpenTelemetry attribute.
fn grpc_status_code(kind: &GrpcError) -> i64 {
    match kind {
        GrpcError::Cancelled => 1,
        GrpcError::Unknown(_) => 2,
        GrpcError::InvalidArgument => 3,
        GrpcError::DeadlineExceeded => 4,
        GrpcError::NotFound => 5,
        GrpcError::AlreadyExists => 6,
        GrpcError::PermissionDenied => 7,
        GrpcError::ResourceExhausted => 8,
        GrpcError::FailedPrecondition => 9,
        GrpcError::Aborted => 10,
        GrpcError::OutOfRange => 11,
        GrpcError::Unimplemented => 12,
        GrpcError::Internal => 13,
        GrpcError::Unavailable => 14,
        GrpcError::DataLoss => 15,
        GrpcError::Unauthenticated => 16,
    }
}

/// Removes cached P2ID notes older than [`NOTE_RETENTION_BLOCKS`], keeping the cache bounded.
///
/// `current_block` is the chain tip. A note's `after_block_num` is the tip when it was cached, so
/// it works as the note's age.
fn prune_stale_p2id_notes(cache: &mut HashMap<String, CachedP2idNote>, current_block: BlockNumber) {
    let threshold = current_block.saturating_sub(NOTE_RETENTION_BLOCKS);
    cache.retain(|_, cached| cached.after_block_num >= threshold);
}

/// Checks that `operator_account_id` is the owner of `faucet_account`.
///
/// # Errors
///
/// Returns an error if the faucet account carries no ownership data (i.e. it is not an
/// owner-controlled faucet), if it has renounced its ownership, or if its owner is some account
/// other than `operator_account_id`.
fn check_faucet_owner_matches_operator(
    faucet_account: &Account,
    operator_account_id: AccountId,
) -> anyhow::Result<()> {
    let faucet_account_id = faucet_account.id();

    let owner = Ownable2Step::try_from_storage(faucet_account.storage())
        .with_context(|| {
            format!(
                "faucet account {faucet_account_id} has no ownership data; \
                 it is not an owner-controlled faucet"
            )
        })?
        .owner()
        .with_context(|| format!("faucet account {faucet_account_id} has no owner"))?;

    anyhow::ensure!(
        owner == operator_account_id,
        "the imported operator account is not the owner of faucet account {faucet_account_id}: \
         the faucet is owned by {owner}, but the account file supplied {operator_account_id}",
    );

    info!(
        faucet_account_id = %faucet_account_id,
        operator_account_id = %owner,
        "Verified the operator account owns the faucet account",
    );

    Ok(())
}

fn build_p2id_notes(
    source: &FaucetId,
    requests: &[MintRequest],
    rng: &mut RandomCoin,
) -> Result<Vec<Note>, NoteError> {
    // If building a note fails, we discard the whole batch. Should never happen, since account
    // ids are validated on the request level.
    let mut notes = Vec::new();
    for request in requests {
        // Match the asset `mint_and_send` mints, so the local note id matches on-chain.
        // SAFETY: source is definitely a faucet account, and the amount is valid.
        let asset =
            FungibleAsset::new(source.account_id, request.asset_amount.base_units()).unwrap();
        let note = Note::from(
            P2idNote::builder()
                .sender(source.account_id)
                .target(request.account_id)
                .asset(asset)
                .note_type(request.note_type.into())
                .generate_serial_number(rng)
                .build()
        .inspect_err(
            |err| error!(request.account_id=%request.account_id, ?err, "failed to build note"),
        )?);
        notes.push(note);
    }
    Ok(notes)
}

fn build_mint_notes(
    faucet_id: AccountId,
    p2id_notes: &[Note],
    rng: &mut RandomCoin,
    faucet_operator_id: AccountId,
) -> Result<Vec<Note>, NoteError> {
    let mut mint_notes = Vec::new();
    for p2id_note in p2id_notes {
        let recipient = p2id_note.recipient().clone();
        let tag = p2id_note.metadata().tag();
        // SAFETY: `build_p2id_notes` builds these with exactly one fungible asset.
        let asset = p2id_note.assets().iter().next().unwrap().unwrap_fungible();

        let storage = match p2id_note.metadata().note_type() {
            ProtocolNoteType::Public => {
                MintNoteStorage::new_fungible_public(recipient, asset, tag)?
            },
            ProtocolNoteType::Private => {
                MintNoteStorage::new_fungible_private(recipient.digest(), asset, tag)
            },
        };
        // SAFETY: `faucet_id` is a public (network) account
        let attachment = NetworkAccountTarget::new(faucet_id, NoteExecutionHint::Always)
            .expect("faucet account type should be public");
        let mint_note = MintNote::builder()
            .sender(faucet_operator_id)
            .mint_storage(storage)
            .attachment(attachment)
            .generate_serial_number(rng)
            .build()?;

        mint_notes.push(mint_note.into());
    }
    Ok(mint_notes)
}

/// Reads the ID of the faucet whose asset the chain charges fees in from the genesis block header.
pub async fn fetch_fee_faucet_id(
    node_endpoint: &Endpoint,
    timeout: Duration,
) -> anyhow::Result<AccountId> {
    let (genesis, _) = GrpcClient::new(node_endpoint, timeout.as_millis() as u64)
        .get_block_header_by_number(Some(BlockNumber::GENESIS), false)
        .await
        .context("failed to fetch the genesis block header")?;

    Ok(genesis.fee_parameters().fee_faucet_id())
}

/// Creates a new network faucet account from the given parameters.
pub fn create_network_faucet_account(
    token_symbol: &str,
    max_supply: u64,
    decimals: u8,
    owner: AccountId,
    fee_faucet_id: AccountId,
) -> anyhow::Result<Account> {
    let symbol = TokenSymbol::try_from(token_symbol).context("failed to parse token symbol")?;
    let name = TokenName::new(&symbol.to_string()).context("failed to derive token name")?;

    let faucet = FungibleFaucet::builder()
        .name(name)
        .symbol(symbol)
        .decimals(decimals)
        .max_supply(
            ProtocolAssetAmount::new(max_supply)
                .context("max supply exceeds the maximum asset amount")?,
        )
        .build()
        .context("failed to build fungible faucet component")?;

    let access_control = AccessControl::Ownable2Step { owner };

    let token_policy_manager = TokenPolicyManager::builder()
        .active_mint_policy(MintPolicy::owner_only())
        .active_burn_policy(BurnPolicy::allow_all())
        .active_send_policy(TransferPolicy::allow_all())
        .active_receive_policy(TransferPolicy::allow_all())
        .build();

    let mut rng = rand::rng();
    let fee_policy = BasicConstantFeePolicy::new()
        .with_fees([
            (MintNote::script_root(), ProtocolAssetAmount::ZERO),
            (BurnNote::script_root(), ProtocolAssetAmount::ZERO),
        ])
        .into();
    let fee_policy_manager = FeePolicyManager::builder()
        .fee_faucet_id(fee_faucet_id)
        .active_fee_policy(fee_policy)
        .build();
    let account = create_network_fungible_faucet(
        rng.random(),
        faucet,
        access_control,
        token_policy_manager,
        fee_policy_manager,
    )
    .context("failed to create basic fungible faucet account")?;

    Ok(account)
}

/// Creates a new operator account.
/// Returns a tuple containing the operator account and its secret key.
pub fn create_faucet_operator_account() -> anyhow::Result<(Account, AuthSecretKey)> {
    let mut rng = rand::rng();
    let secret_key = {
        let auth_seed: [u64; 4] = rng.random();
        let rng_seed = Word::from(auth_seed.map(Felt::new_unchecked));
        SecretKey::with_rng(&mut RandomCoin::new(rng_seed))
    };

    let auth_component: AccountComponent = AuthSingleSig::new(Approver::new(
        secret_key.public_key().into(),
        AuthScheme::Falcon512Poseidon2,
    ))
    .into();

    let init_seed = rng.random();
    let account = AccountBuilder::new(init_seed)
        .account_type(AccountType::Public)
        .with_component(auth_component)
        .with_component(BasicWallet)
        .build()?;

    Ok((account, AuthSecretKey::Falcon512Poseidon2(secret_key)))
}

/// Whether `details` describes a P2ID or P2IDE note payable to `target` that carries
/// `fee_faucet_id`'s asset.
pub(crate) fn is_note_payable_to(
    details: &NoteDetails,
    target: AccountId,
    fee_faucet_id: AccountId,
) -> bool {
    let assets = details.assets();
    let carries_fee_asset = !assets.is_empty()
        && assets
            .iter()
            .all(|asset| asset.is_fungible() && asset.faucet_id() == fee_faucet_id);

    carries_fee_asset
        && (is_p2id_payable_to(details, target) || is_p2ide_payable_to(details, target))
}

/// Whether `details` describes a P2ID note payable to `target`.
fn is_p2id_payable_to(details: &NoteDetails, target: AccountId) -> bool {
    *details.recipient() == P2idNoteStorage::new(target).into_recipient(details.serial_num())
}

/// Whether `details` describes a P2IDE note payable to `target`.
fn is_p2ide_payable_to(details: &NoteDetails, target: AccountId) -> bool {
    let recipient = details.recipient();
    if recipient.script().root() != P2ideNote::script_root() {
        return false;
    }

    P2ideNoteStorage::try_from(recipient.storage().items())
        .is_ok_and(|storage| storage.target() == target)
}

/// Whether `details` describes a note that can be consumed by its target at `block_num`.
///
/// A P2IDE note may be timelocked, in which case no transaction can consume it until its timelock
/// height has passed. A P2ID note carries no such condition.
pub(crate) fn is_consumable_at(details: &NoteDetails, block_num: BlockNumber) -> bool {
    let recipient = details.recipient();
    if recipient.script().root() == P2idNote::script_root() {
        return true;
    }

    P2ideNoteStorage::try_from(recipient.storage().items())
        .is_ok_and(|storage| storage.timelock_height().is_none_or(|height| height <= block_num))
}

#[cfg(test)]
mod tests {
    use std::env::temp_dir;

    use miden_client::asset::{Asset, AssetId};
    use miden_client::block::BlockNumber;
    use miden_client::crypto::eddsa_25519_sha512::KeyExchangeKey;
    use miden_client::rpc::encryption::TransactionEncryptionKey;
    use miden_client::store::Store;
    use miden_client::testing::account_id::{
        ACCOUNT_ID_FEE_FAUCET,
        ACCOUNT_ID_REGULAR_PUBLIC_ACCOUNT_IMMUTABLE_CODE,
    };
    use miden_client::testing::mock::MockRpcApi;
    use miden_client::testing::{Auth, MockChainBuilder};
    use miden_client::transaction::RawOutputNote;
    use tokio::sync::{mpsc, oneshot};
    use uuid::Uuid;

    use super::*;
    use crate::types::NoteType;

    /// How many base units of the chain's fee asset the test sender wallet holds at genesis.
    const SENDER_BALANCE: u64 = 1_000_000_000_000;

    /// Only notes older than the retention window are pruned, and the boundary itself is kept.
    #[test]
    fn prunes_only_notes_past_the_retention_window() {
        let current_block = BlockNumber::from(NOTE_RETENTION_BLOCKS + 10);
        let note = p2id_note();

        // One note per age: too old, exactly at the oldest kept block, and current.
        let mut cache = HashMap::new();
        for after_block_num in [
            BlockNumber::from(9),
            current_block.saturating_sub(NOTE_RETENTION_BLOCKS),
            current_block,
        ] {
            cache.insert(
                after_block_num.as_u32().to_string(),
                CachedP2idNote { note: note.clone(), after_block_num },
            );
        }

        prune_stale_p2id_notes(&mut cache, current_block);

        assert!(!cache.contains_key("9"), "a note past the window should be pruned");
        assert!(
            cache.contains_key("10"),
            "a note exactly at the oldest kept block should be kept"
        );
        assert!(cache.contains_key(&current_block.as_u32().to_string()));
    }

    /// Pruning from a chain younger than the retention window must not underflow.
    #[test]
    fn prunes_nothing_before_the_window_has_elapsed() {
        let note = p2id_note();
        let mut cache = HashMap::from([(
            "genesis".to_owned(),
            CachedP2idNote {
                note,
                after_block_num: BlockNumber::GENESIS,
            },
        )]);

        prune_stale_p2id_notes(&mut cache, BlockNumber::from(5));

        assert_eq!(cache.len(), 1, "nothing is old enough to prune yet");
    }

    #[tokio::test]
    async fn batch_requests() {
        let batch_size = 32;

        let (tx_mint_requests, rx_mint_requests) = mpsc::channel(1000);
        let mut receivers = vec![];
        for i in 0..batch_size {
            let (sender, receiver) = oneshot::channel();
            let mint_request = MintRequest {
                account_id: AccountId::try_from(ACCOUNT_ID_REGULAR_PUBLIC_ACCOUNT_IMMUTABLE_CODE)
                    .unwrap(),
                note_type: if i % 2 == 0 {
                    NoteType::Public
                } else {
                    NoteType::Private
                },
                asset_amount: AssetAmount::new(100_000_000).unwrap(),
            };
            tx_mint_requests.send((mint_request, sender)).await.unwrap();
            receivers.push(receiver);
        }
        // Close channel after all requests are sent
        drop(tx_mint_requests);

        let store = Arc::new(
            SqliteStore::new(temp_dir().join(format!("{}.sqlite3", Uuid::new_v4())))
                .await
                .unwrap(),
        );
        let mut faucet = build_faucet(store.clone()).await;
        faucet.run(rx_mint_requests, batch_size).await.unwrap();

        // Requests alternate public/private, and `receivers` preserves that order. Only the private
        // notes are cached; a public note's details are on chain, so the faucet needn't keep them.
        for (i, receiver) in receivers.into_iter().enumerate() {
            let response = receiver.await.unwrap().unwrap();
            let cached = faucet.get_p2id_note(response.note_id);
            if i % 2 == 0 {
                assert!(cached.is_none(), "public note {i} should not be cached");
            } else {
                assert!(cached.is_some(), "private note {i} should be cached");
            }
        }
    }

    /// When the operator account balance is below `OPERATOR_FUNDING_THRESHOLD`, the faucet mints
    /// a P2ID note to fund the operator, when a new mint request arrives. The P2ID note is consumed
    /// in the next mint batch, since the faucet includes that note as part of the mint transaction.
    #[tokio::test]
    async fn mint_funds_the_operator_when_its_balance_is_low() {
        let store = Arc::new(
            SqliteStore::new(temp_dir().join(format!("{}.sqlite3", Uuid::new_v4())))
                .await
                .unwrap(),
        );
        // Set the initial operator balance below the threshold so that the next mint request
        // triggers the funding mechanism
        let initial_operator_balance = OPERATOR_FUNDING_THRESHOLD - 1;
        let verification_base_fee = 0;
        let faucet_is_chain_fee_faucet = true;
        let (mut faucet, mock_rpc, _) = build_faucet_on_chain(
            store.clone(),
            verification_base_fee,
            initial_operator_balance,
            faucet_is_chain_fee_faucet,
        )
        .await;

        // Check the faucet asset matches the chain's native asset
        let fee_parameters = faucet.fee_parameters().await.unwrap();
        assert_eq!(fee_parameters.fee_faucet_id(), faucet.faucet_id().account_id);
        assert_eq!(operator_fee_balance(&faucet, &fee_parameters).await, initial_operator_balance);
        assert!(
            faucet.operator_requires_funding().await.unwrap(),
            "the operator sits one base unit below the threshold"
        );

        // Send and execute a mint request. Since the operator's balance is below the threshold,
        // the faucet mint transaction will include an extra MINT note to fund the operator.
        let response = send_and_execute_mint_request(&mut faucet, mint_request()).await;
        // The operator balance has not changed yet, since the P2ID note will be consumed when
        // executing the next batch of mint requests.
        assert_eq!(operator_fee_balance(&faucet, &fee_parameters).await, initial_operator_balance);
        assert!(faucet.funding_request_in_flight, "the funding request is in flight");

        // Get the MINT notes from the faucet transaction
        let mint_note_ids = get_tx_output_note_ids(&faucet.client, response.tx_id).await;
        // The mock chain does not support network transactions so we need to manually consume
        // the MINT notes against the faucet account
        execute_network_tx_with_notes(
            &mock_rpc,
            mint_note_ids,
            faucet.faucet_account().await.unwrap().id(),
        )
        .await;

        // Send and execute another mint request. In this case, the client will find and consume
        // the P2ID note that funds the operator, and therefore its balance will increase
        send_and_execute_mint_request(&mut faucet, mint_request()).await;
        assert_eq!(
            operator_fee_balance(&faucet, &fee_parameters).await,
            initial_operator_balance + OPERATOR_FUNDING_AMOUNT,
            "consuming the funding note should raise the operator's balance"
        );
        assert!(!faucet.funding_request_in_flight, "the funding request is no longer in flight");
    }

    /// A P2IDE note anyone sends the operator in the chain's fee asset is picked up by the sync
    /// and consumed by the next mint, raising the operator's balance.
    #[tokio::test]
    async fn operator_consumes_a_p2ide_note_sent_to_it() {
        let transferred = 250_000;
        let store = Arc::new(
            SqliteStore::new(temp_dir().join(format!("{}.sqlite3", Uuid::new_v4())))
                .await
                .unwrap(),
        );
        let (mut faucet, mock_rpc, sender) =
            build_faucet_on_chain(store.clone(), 0, OPERATOR_FUNDING_THRESHOLD, true).await;

        send_p2ide_note_to_operator(&mut faucet, &mock_rpc, &sender, transferred).await;
        // The P2IDE note will be consumed in the next mint request
        send_and_execute_mint_request(&mut faucet, mint_request()).await;

        let fee_parameters = faucet.fee_parameters().await.unwrap();
        assert_eq!(
            operator_fee_balance(&faucet, &fee_parameters).await,
            OPERATOR_FUNDING_THRESHOLD + transferred,
            "the mint should have consumed the P2IDE note paying the operator"
        );
    }

    // FEE TESTS
    // ---------------------------------------------------------------------------------------------

    /// The base fee the fee tests charge. The kernel charges it once per verification cycle
    /// (`ilog2(execution cycles) + 1`), so a transaction pays a small multiple of it.
    const TEST_VERIFICATION_BASE_FEE: u32 = 500;

    /// The MINT transaction declares the faucet as a foreign account, requesting every entry of
    /// each of its map slots.
    #[test]
    fn mint_transaction_request_shape() {
        let fee_faucet_id = AccountId::try_from(ACCOUNT_ID_FEE_FAUCET).unwrap();
        let (operator, _) = create_faucet_operator_account().unwrap();
        let faucet =
            create_network_faucet_account("TEST", 1_000, 6, operator.id(), fee_faucet_id).unwrap();
        let mint_note = mint_note(&faucet, operator.id());

        let foreign_account = faucet_foreign_account(&faucet).unwrap();
        let map_slots: Vec<_> = faucet
            .storage()
            .slots()
            .iter()
            .filter(|slot| matches!(slot.content(), StorageSlotContent::Map(_)))
            .map(|slot| slot.name().clone())
            .collect();
        assert!(!map_slots.is_empty(), "a network faucet has storage maps");
        let requirements = foreign_account.storage_slot_requirements();
        assert_eq!(requirements.inner().len(), map_slots.len());
        for slot in &map_slots {
            assert!(
                requirements.keys_for_slot(slot).is_empty(),
                "slot {slot} should request all entries, without proofs"
            );
        }
        // Input notes are only present when the operator consumes P2ID notes
        let input_notes = vec![];
        let request = Faucet::create_transaction(
            std::slice::from_ref(&mint_note),
            &input_notes,
            foreign_account,
        )
        .unwrap();
        assert!(request.foreign_accounts().contains_key(&faucet.id()));
        assert_eq!(request.expected_output_own_notes(), vec![mint_note.clone()]);
    }

    /// On a fee-charging chain, a funded operator pays the MINT transaction fee: the transaction
    /// emits a `TX_FEE` note funded from the operator's vault.
    #[tokio::test]
    async fn mint_pays_fee_on_fee_charging_chain() {
        let operator_fee_balance = 1_000_000;
        let store = Arc::new(
            SqliteStore::new(temp_dir().join(format!("{}.sqlite3", Uuid::new_v4())))
                .await
                .unwrap(),
        );
        let (mut faucet, ..) = build_faucet_on_chain(
            store.clone(),
            TEST_VERIFICATION_BASE_FEE,
            operator_fee_balance,
            false,
        )
        .await;
        let fee_parameters = faucet.fee_parameters().await.unwrap();
        let fee_asset_id = AssetId::new_fungible(fee_parameters.fee_faucet_id());

        let (tx_mint_requests, rx_mint_requests) = mpsc::channel(1);
        let (sender, receiver) = oneshot::channel();
        tx_mint_requests.send((mint_request(), sender)).await.unwrap();
        drop(tx_mint_requests);
        faucet.run(rx_mint_requests, 1).await.unwrap();

        let response = receiver.await.unwrap().expect("a funded operator can mint");

        // The operator paid the fee out of its vault.
        let operator_reader = faucet.client.account_reader(faucet.operator_id());
        let balance = fee_asset_balance(&operator_reader, &fee_parameters).await.unwrap();
        let fee_paid = operator_fee_balance - balance;
        assert!(fee_paid > 0, "the operator should have paid a fee");
        // The base fee is charged once per verification cycle.
        assert_eq!(fee_paid % u64::from(TEST_VERIFICATION_BASE_FEE), 0);

        // The fee left the transaction in a TX_FEE note carrying exactly the paid amount.
        let transaction = faucet
            .client
            .get_transactions(TransactionFilter::Ids(vec![response.tx_id]))
            .await
            .unwrap()
            .pop()
            .expect("the mint transaction is tracked");
        // The fee asset left the transaction in two notes: the TX_FEE note paying for it, and the
        // sponsorship prepaying the network transaction that consumes the MINT note.
        let mut fee_asset_amounts: Vec<u64> = transaction
            .details
            .output_notes
            .iter()
            .flat_map(|note| note.assets().iter())
            .filter(|asset| asset.id() == fee_asset_id)
            .map(|asset| asset.unwrap_fungible().amount().as_u64())
            .collect();
        fee_asset_amounts.sort_unstable();
        let sponsored = sponsorship_amount(&fee_parameters);
        let mut expected = vec![sponsored, fee_paid - sponsored];
        expected.sort_unstable();
        assert_eq!(fee_asset_amounts, expected);
    }

    /// A faucet collecting in the chain's native fee asset gets one sponsorship per MINT note; one
    /// collecting in its operator's asset - what genesis produces today - gets none, since the node
    /// would drop them.
    #[test]
    fn sponsorships_require_the_native_fee_asset() {
        let fee_faucet_id = AccountId::try_from(ACCOUNT_ID_FEE_FAUCET).unwrap();
        let fee_parameters = FeeParameters::new(fee_faucet_id, TEST_VERIFICATION_BASE_FEE);
        let (operator, _) = create_faucet_operator_account().unwrap();
        let mut rng = RandomCoin::new(Word::empty());

        let mut sponsorships = |fee_collected_in| {
            let faucet =
                create_network_faucet_account("TEST", 1_000, 6, operator.id(), fee_collected_in)
                    .unwrap();
            let mint_notes = [mint_note(&faucet, operator.id())];
            build_sponsorship_notes(
                operator.id(),
                &faucet,
                &mint_notes,
                &fee_parameters,
                BlockNumber::from(SPONSORSHIP_RECLAIM_DELTA),
                &mut rng,
            )
            .unwrap()
        };

        let native = sponsorships(fee_faucet_id);
        assert_eq!(native.len(), 1);
        assert_eq!(
            native[0].assets().iter().next().unwrap().unwrap_fungible().amount().as_u64(),
            sponsorship_amount(&fee_parameters),
        );
        assert!(sponsorships(operator.id()).is_empty());
    }

    // TESTING HELPERS
    // ---------------------------------------------------------------------------------------------

    /// The faucet's max supply in the tests, with room for several funding mints on top of the
    /// requests a batch carries.
    const TEST_MAX_SUPPLY: u64 = OPERATOR_FUNDING_AMOUNT * 10;

    /// A mint request for a public note to a fixed account.
    fn mint_request() -> MintRequest {
        MintRequest {
            account_id: AccountId::try_from(ACCOUNT_ID_REGULAR_PUBLIC_ACCOUNT_IMMUTABLE_CODE)
                .unwrap(),
            note_type: NoteType::Public,
            asset_amount: AssetAmount::new(100_000_000).unwrap(),
        }
    }

    /// Builds a MINT note for `faucet`, sent by `operator_id`, the way the faucet does.
    /// The amount a MINT note's sponsorship carries under `fee_parameters`.
    fn sponsorship_amount(fee_parameters: &FeeParameters) -> u64 {
        NetworkNotePricer::builder()
            .fee_parameters(fee_parameters.clone())
            .build()
            .price(MintNote::script_root())
            .unwrap()
            .as_u64()
    }

    fn mint_note(faucet: &Account, operator_id: AccountId) -> Note {
        let faucet_id = FaucetId::new(faucet.id(), NetworkId::Testnet);
        let mut rng = RandomCoin::new(Word::empty());
        let p2id_notes = build_p2id_notes(&faucet_id, &[mint_request()], &mut rng).unwrap();
        build_mint_notes(faucet.id(), &p2id_notes, &mut rng, operator_id)
            .unwrap()
            .remove(0)
    }

    /// Builds a faucet using a mock client on a chain that charges no fees.
    async fn build_faucet(store: Arc<dyn Store>) -> Faucet {
        build_faucet_on_chain(store, 0, 0, false).await.0
    }

    /// Builds a faucet using a mock client on a chain charging `verification_base_fee`, with the
    /// operator holding `operator_fee_balance` base units of the chain's fee asset at genesis.
    ///
    /// `faucet_is_chain_fee_faucet` makes the chain charge fees in the faucet's own asset, the only
    /// arrangement in which the faucet can fund its operator.
    ///
    /// Returns a tuple containing the faucet, the RPC api, and a funded account.
    async fn build_faucet_on_chain(
        store: Arc<dyn Store>,
        verification_base_fee: u32,
        operator_fee_balance: u64,
        faucet_is_chain_fee_faucet: bool,
    ) -> (Faucet, Arc<MockRpcApi>, Account) {
        let (mut operator_account, operator_secret) = create_faucet_operator_account().unwrap();
        let symbol = "TEST";
        let decimals = 6;
        let max_supply = TEST_MAX_SUPPLY;
        let fee_faucet_id = AccountId::try_from(ACCOUNT_ID_FEE_FAUCET).unwrap();
        let faucet_account = create_network_faucet_account(
            symbol,
            max_supply,
            decimals,
            operator_account.id(),
            fee_faucet_id,
        )
        .unwrap();

        let keystore_path = temp_dir().join(format!("keystore-{}", Uuid::new_v4()));
        let keystore = FilesystemKeyStore::new(keystore_path.clone()).unwrap();
        keystore.add_key(&operator_secret, operator_account.id()).await.unwrap();

        // The operator's mint transaction reads the faucet account via FPI, and foreign account
        // inputs are always fetched over RPC, so the chain must have it committed. The chain
        // builder only takes deployed accounts, so commit it at nonce 1, which is the state
        // `Faucet::init` leaves it in after the deployment transaction.
        let mut deployed_faucet = faucet_account.clone();
        deployed_faucet.set_nonce(Felt::new_unchecked(1)).unwrap();
        // The operator is committed as well, with its fee asset balance, mirroring an operator
        // funded on chain before the faucet starts.
        let chain_fee_faucet_id = if faucet_is_chain_fee_faucet {
            faucet_account.id()
        } else {
            fee_faucet_id
        };
        if operator_fee_balance > 0 {
            let fee_asset = FungibleAsset::new(chain_fee_faucet_id, operator_fee_balance).unwrap();
            operator_account.vault_mut().add_asset(fee_asset.into()).unwrap();
        }
        operator_account.set_nonce(Felt::new_unchecked(1)).unwrap();
        let mut chain_builder =
            MockChainBuilder::with_accounts([deployed_faucet, operator_account.clone()])
                .unwrap()
                .fee_faucet_id(chain_fee_faucet_id)
                .verification_base_fee(verification_base_fee);
        // A wallet holding the chain's fee asset, so tests can send notes to the operator.
        let sender = chain_builder
            .add_existing_wallet_with_assets(
                Auth::IncrNonce,
                [Asset::from(FungibleAsset::new(chain_fee_faucet_id, SENDER_BALANCE).unwrap())],
            )
            .unwrap();
        let mock_chain = chain_builder.build().unwrap();
        let fee_parameters = mock_chain.latest_block_header().fee_parameters().clone();
        assert_eq!(fee_parameters.verification_base_fee(), verification_base_fee);
        let mock_rpc = Arc::new(MockRpcApi::new(mock_chain));
        let mut client = ClientBuilder::new()
            .rpc(mock_rpc.clone())
            .store(store.clone())
            .filesystem_keystore(keystore_path.to_str().unwrap())
            .expect("keystore should be created")
            .build()
            .await
            .unwrap();
        client.ensure_genesis_in_place().await.unwrap();
        client.add_account(&faucet_account, false).await.unwrap();
        client.add_account(&operator_account, false).await.unwrap();
        client.add_account(&sender, false).await.unwrap();
        // `Faucet::init` records both accounts as settings; the note screener reads the operator
        // from there.
        client
            .set_setting(DEFAULT_ACCOUNT_ID_SETTING.to_owned(), faucet_account.id())
            .await
            .unwrap();
        client
            .set_setting(DEFAULT_OPERATOR_ACCOUNT_ID_SETTING.to_owned(), operator_account.id())
            .await
            .unwrap();

        // The mock RPC serves no transaction encryption key, so seed an unattested one:
        // submission seals against it and the mock node ignores the sealed payload.
        let genesis_commitment = client
            .get_block_header_by_num(BlockNumber::GENESIS)
            .await
            .unwrap()
            .expect("genesis header must be in place")
            .0
            .commitment();
        client
            .seed_transaction_encryption_key(TransactionEncryptionKey::new_unattested(
                b"mock-key-id".to_vec(),
                KeyExchangeKey::new().public_key(),
                genesis_commitment,
            ))
            .await
            .unwrap();

        let (issuance, _) = watch::channel(AssetAmount::new(0).unwrap());
        let faucet = Faucet {
            id: FaucetId::new(faucet_account.id(), NetworkId::Testnet),
            client,
            state_sync_component: StateSync::new(
                mock_rpc.clone(),
                Arc::new(NoteScreener::new(store)),
                None,
            ),
            tx_prover: Arc::new(LocalTransactionProver::default()),
            issuance,
            max_supply: AssetAmount::new(TEST_MAX_SUPPLY).unwrap(),
            operator_account_id: operator_account.id(),
            p2id_notes: P2idNoteCache::default(),
            funding_request_in_flight: false,
        };
        (faucet, mock_rpc, sender)
    }

    /// Runs a single-request batch and returns the mint response.
    async fn send_and_execute_mint_request(
        faucet: &mut Faucet,
        mint_request: MintRequest,
    ) -> MintResponse {
        let (tx_mint_requests, rx_mint_requests) = mpsc::channel(1);
        let (response_sender, receiver) = oneshot::channel();
        tx_mint_requests.send((mint_request, response_sender)).await.unwrap();
        drop(tx_mint_requests);

        faucet.run(rx_mint_requests, 1).await.unwrap();
        receiver.await.unwrap().unwrap()
    }

    /// Executes a transaction against a network account, consuming the notes given by
    /// `input_note_ids`. This is intended to be used in tests that require executing network
    /// transactions.
    async fn execute_network_tx_with_notes(
        mock_rpc: &MockRpcApi,
        input_note_ids: Vec<NoteId>,
        account_id: AccountId,
    ) {
        // The notes have to be committed before they can be consumed as authenticated inputs.
        mock_rpc.prove_block();

        let network_tx = {
            let chain = mock_rpc.mock_chain.read();
            chain
                .build_transaction(account_id)
                .authenticated_input_notes(input_note_ids)
                .build()
                .unwrap()
        };
        let executed = network_tx.execute().await.unwrap();

        mock_rpc.mock_chain.write().add_pending_executed_transaction(&executed).unwrap();
        mock_rpc.prove_block();
    }

    /// Returns the IDs of the output notes created in the transaction given by `tx_id`.
    async fn get_tx_output_note_ids(
        client: &Client<FilesystemKeyStore>,
        tx_id: TransactionId,
    ) -> Vec<NoteId> {
        client
            .get_transactions(TransactionFilter::Ids(vec![tx_id]))
            .await
            .unwrap()
            .pop()
            .expect("the mint transaction is tracked")
            .details
            .output_notes
            .iter()
            .map(RawOutputNote::id)
            .collect()
    }

    /// Commits a P2IDE note paying the operator `amount` of the chain's fee asset, in a new block.
    ///
    /// The transaction is executed through the faucet's client, since it is the only client the
    /// test has, but it is never applied to its store. The note therefore reaches the faucet only
    /// through the sync, like a transfer made by anyone else would.
    async fn send_p2ide_note_to_operator(
        faucet: &mut Faucet,
        mock_rpc: &MockRpcApi,
        sender: &Account,
        amount: u64,
    ) {
        let fee_faucet_id = faucet.fee_parameters().await.unwrap().fee_faucet_id();
        let note: Note = P2ideNote::builder()
            .sender(sender.id())
            .target(faucet.operator_id())
            .asset(FungibleAsset::new(fee_faucet_id, amount).unwrap())
            .note_type(ProtocolNoteType::Public)
            .serial_number(Word::from([9u32; 4]))
            .build()
            .unwrap()
            .into();

        let tx_request = TransactionRequestBuilder::new().own_output_notes([note]).build().unwrap();
        let tx = faucet.client.execute_transaction(sender.id(), tx_request).await.unwrap();

        mock_rpc
            .mock_chain
            .write()
            .add_pending_executed_transaction(tx.executed_transaction())
            .unwrap();
        mock_rpc.prove_block();
    }

    /// Returns the operator's balance of the chain's fee asset, as tracked by the faucet's client.
    async fn operator_fee_balance(faucet: &Faucet, fee_parameters: &FeeParameters) -> u64 {
        let operator = faucet.client.account_reader(faucet.operator_id());
        fee_asset_balance(&operator, fee_parameters).await.unwrap()
    }

    /// Builds an arbitrary P2ID note; only its presence matters to the pruning tests.
    fn p2id_note() -> Note {
        let target = AccountId::try_from(ACCOUNT_ID_REGULAR_PUBLIC_ACCOUNT_IMMUTABLE_CODE).unwrap();
        P2idNote::builder()
            .sender(target)
            .target(target)
            .asset(FungibleAsset::new(target, 1).unwrap())
            .serial_number(Word::empty())
            .build()
            .unwrap()
            .into()
    }
}
