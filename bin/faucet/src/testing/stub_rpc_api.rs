use anyhow::Context;
use miden_node_proto::generated::rpc::api_server;
use miden_node_proto::generated::{self as proto};
use miden_testing::MockChain;
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::{Request, Response, Status};
use tonic_web::GrpcWebLayer;
use url::Url;

pub struct StubRpcApi;

#[tonic::async_trait]
impl api_server::Api for StubRpcApi {
    async fn get_block_header_by_number(
        &self,
        _request: Request<proto::rpc::BlockHeaderByNumberRequest>,
    ) -> Result<Response<proto::rpc::BlockHeaderByNumberResponse>, Status> {
        let mock_chain = MockChain::new();

        Ok(Response::new(proto::rpc::BlockHeaderByNumberResponse {
            block_header: Some(mock_chain.latest_block_header().into()),
            mmr_path: None,
            chain_length: None,
        }))
    }

    async fn sync_notes(
        &self,
        _request: Request<proto::rpc::SyncNotesRequest>,
    ) -> Result<Response<proto::rpc::SyncNotesResponse>, Status> {
        Ok(Response::new(proto::rpc::SyncNotesResponse {
            pagination_info: Some(proto::rpc::PaginationInfo { chain_tip: 0, block_num: 0 }),
            blocks: vec![],
        }))
    }

    async fn get_notes_by_id(
        &self,
        _request: Request<proto::note::NoteIdList>,
    ) -> Result<Response<proto::note::CommittedNoteList>, Status> {
        unimplemented!()
    }

    async fn submit_proven_tx(
        &self,
        _request: Request<proto::transaction::ProvenTransaction>,
    ) -> Result<Response<proto::blockchain::BlockNumber>, Status> {
        Ok(Response::new(proto::blockchain::BlockNumber { block_num: 0 }))
    }

    async fn submit_proven_tx_batch(
        &self,
        _request: Request<proto::transaction::TransactionBatch>,
    ) -> Result<Response<proto::blockchain::BlockNumber>, Status> {
        unimplemented!()
    }

    type BlockSubscriptionStream = std::pin::Pin<
        Box<
            dyn tokio_stream::Stream<Item = Result<proto::rpc::BlockSubscriptionResponse, Status>>
                + Send,
        >,
    >;

    async fn block_subscription(
        &self,
        _request: Request<proto::rpc::BlockSubscriptionRequest>,
    ) -> Result<Response<Self::BlockSubscriptionStream>, Status> {
        unimplemented!()
    }

    type ProofSubscriptionStream = std::pin::Pin<
        Box<
            dyn tokio_stream::Stream<Item = Result<proto::rpc::ProofSubscriptionResponse, Status>>
                + Send,
        >,
    >;

    async fn proof_subscription(
        &self,
        _request: Request<proto::rpc::ProofSubscriptionRequest>,
    ) -> Result<Response<Self::ProofSubscriptionStream>, Status> {
        unimplemented!()
    }

    async fn get_account(
        &self,
        _request: Request<proto::rpc::AccountRequest>,
    ) -> Result<Response<proto::rpc::AccountResponse>, Status> {
        Err(Status::not_found("account not found"))
    }

    async fn get_block_by_number(
        &self,
        _request: Request<proto::blockchain::BlockRequest>,
    ) -> Result<Response<proto::blockchain::MaybeBlock>, Status> {
        unimplemented!()
    }

    async fn status(
        &self,
        _request: Request<()>,
    ) -> Result<Response<proto::rpc::RpcStatus>, Status> {
        unimplemented!()
    }

    async fn sync_account_vault(
        &self,
        _request: Request<proto::rpc::SyncAccountVaultRequest>,
    ) -> Result<Response<proto::rpc::SyncAccountVaultResponse>, Status> {
        unimplemented!()
    }

    async fn sync_account_storage_maps(
        &self,
        _request: Request<proto::rpc::SyncAccountStorageMapsRequest>,
    ) -> Result<Response<proto::rpc::SyncAccountStorageMapsResponse>, Status> {
        unimplemented!()
    }

    async fn get_note_script_by_root(
        &self,
        _request: Request<proto::note::NoteScriptRoot>,
    ) -> Result<Response<proto::rpc::MaybeNoteScript>, Status> {
        unimplemented!()
    }

    async fn sync_nullifiers(
        &self,
        _request: Request<proto::rpc::SyncNullifiersRequest>,
    ) -> Result<Response<proto::rpc::SyncNullifiersResponse>, Status> {
        Ok(Response::new(proto::rpc::SyncNullifiersResponse {
            nullifiers: vec![],
            pagination_info: Some(proto::rpc::PaginationInfo { chain_tip: 0, block_num: 0 }),
        }))
    }

    async fn sync_transactions(
        &self,
        _request: Request<proto::rpc::SyncTransactionsRequest>,
    ) -> Result<Response<proto::rpc::SyncTransactionsResponse>, Status> {
        unimplemented!()
    }

    async fn get_limits(
        &self,
        _request: Request<()>,
    ) -> Result<Response<proto::rpc::RpcLimits>, Status> {
        use std::collections::HashMap;

        let make_endpoint = |params: Vec<(&str, u32)>| proto::rpc::EndpointLimits {
            parameters: params.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
        };

        let endpoints = HashMap::from([
            ("GetNotesById".to_string(), make_endpoint(vec![("note_id", 100)])),
            ("CheckNullifiers".to_string(), make_endpoint(vec![("nullifier", 1000)])),
            ("SyncNullifiers".to_string(), make_endpoint(vec![("nullifier", 1000)])),
            ("SyncTransactions".to_string(), make_endpoint(vec![("account_id", 1000)])),
            ("SyncNotes".to_string(), make_endpoint(vec![("note_tag", 1000)])),
        ]);

        Ok(Response::new(proto::rpc::RpcLimits { endpoints }))
    }

    async fn sync_chain_mmr(
        &self,
        _request: Request<proto::rpc::SyncChainMmrRequest>,
    ) -> Result<Response<proto::rpc::SyncChainMmrResponse>, Status> {
        let mock_chain = MockChain::new();

        Ok(Response::new(proto::rpc::SyncChainMmrResponse {
            block_range: Some(proto::rpc::BlockRange { block_from: 0, block_to: 0 }),
            mmr_delta: Some(proto::primitives::MmrDelta { forest: 0, data: vec![] }),
            block_header: Some(mock_chain.latest_block_header().into()),
            block_signature: None,
        }))
    }

    async fn get_network_note_status(
        &self,
        _request: Request<proto::note::NoteId>,
    ) -> Result<Response<proto::rpc::GetNetworkNoteStatusResponse>, Status> {
        unimplemented!()
    }
}

pub async fn serve_stub(endpoint: &Url) -> anyhow::Result<()> {
    let addr = endpoint
        .socket_addrs(|| None)
        .context("failed to convert endpoint to socket address")?
        .into_iter()
        .next()
        .unwrap();

    let listener = TcpListener::bind(addr).await?;
    let api_service = api_server::ApiServer::new(StubRpcApi);

    tonic::transport::Server::builder()
        .accept_http1(true)
        .layer(GrpcWebLayer::new())
        .add_service(api_service)
        .serve_with_incoming(TcpListenerStream::new(listener))
        .await
        .context("failed to serve stub RPC API")
}
