# Architecture Overview

This document provides a high-level overview of the Miden faucet architecture.

## System Architecture

The high-level structure of the project looks like follows:

<p style="text-align: center;">
    <img src="../img/architecture.png"/>
</p>

## Core Components

### 1. Web Frontend
- **Technology**: Vanilla JavaScript, HTML5, CSS3
- **Purpose**: User interface for token requests
- **Features**: 
  - Token amount selection
  - Address input
  - PoW challenge solving
  - Request status display

### 2. REST API Server
- **Technology**: Axum HTTP framework
- **Purpose**: HTTP API endpoints for programmatic access
- **Features**:
   - Input parsing and validation
   - Handle account metadata

### 3. Faucet lib
- **Technology**: Rust library (`crates/faucet`)
- **Purpose**: Types shared by the faucet server and the faucet client
- **Features**:
  - Request and response shapes of the HTTP API
  - Asset amount validation

### 4. PoW Rate Limiter
- **Technology**: Rust library (`crates/pow`)
- **Purpose**: Proof of Work rate limiter
- **Features**:
   - PoW challenges issuing, tracking and validation
   - Rate limiting

### 5. Funding service
- **Purpose**: Holds the chain's native asset and creates the notes
- **Features**:
  - Creates a public P2ID note per request and queues it for the next funding transaction
  - Reports its account, balance and per-request maximum through `/status`

The faucet owns no account and submits no transactions. It is the gatekeeper in front of the funding
service: it validates the proof of work, the API key and the claim cap, then forwards the request.
The funding service has no authentication or rate limiting of its own, so its HTTP API must be
reachable only from the faucet.

## Token request Flow

The basic HTTP requests for minting tokens involves `/pow` and `/get_tokens`. These are the entry points for the minting process. This is how the whole flow looks like:

<p style="text-align: center;">
    <img src="../img/flow.png"/>
</p>

### Detailed Flow

- **Request Initiation**
   - User submits token request (web or API)
   - System generates proof-of-work challenge
   - Challenge stored in cache with expiration

- **Challenge Resolution**
   - User solves computational challenge
   - Solution validated against challenge
   - Rate limiting enforced

- **Token Distribution**
   - Validated request forwarded to the funding service
   - The funding service creates a public P2ID note addressed to the recipient

- **Response**
   - Transaction ID and Note ID returned

## Why do we need a backend?

Could the frontend not call the funding service directly? The reason is security: the funding
service has no authentication and no rate limiting, so anyone who can reach it can drain it. The
backend is what enforces the proof of work, the API keys and the claim cap, and it is the only thing
allowed to reach the funding service.

## Security Features

The faucet implements several security measures to prevent abuse:

- **Proof of Work requests**:
  - Users must complete a computational challenge before their request is processed.
  - The challenge difficulty increases with the load. The load is measured by the amount of challenges that were submitted but still haven't expired.
  - Each challenge is signed with a secret only known by the server. It should NOT be shared.
  - **Rate limiting**: if an account submitted a challenge, it can't submit another one until the previous one is expired. The challenge lifetime duration is fixed and set when running the faucet.
  - **API Keys**: the faucet is initialized with a set of API Keys that can be distributed to developers. The difficulty of the challenges requested using the API Key will increase only with the load of that key, it won't be influenced by the overall load of the faucet.

- **Claim cap**: each request is capped by `--max-claimable-amount`, which the faucet refuses to
  start with if it is larger than the funding service's own maximum.
