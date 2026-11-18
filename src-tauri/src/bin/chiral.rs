use chiral_network::{
    consumer::Consumer,
    protocol::{self, InferenceOffer, Quote, Signed},
};
use clap::{Parser, Subcommand};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::{fs, path::PathBuf};

#[derive(Parser)]
#[command(
    name = "chiral",
    about = "Public-testnet inference consumer. Provider prepayments are non-refundable."
)]
struct Cli {
    /// Explicit consumer JSON profile. Never falls back to a legacy chain.
    #[arg(long, env = "CHIRAL_CONSUMER_CONFIG")]
    config: PathBuf,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    Network {
        #[command(subcommand)]
        command: NetworkCommand,
    },
    Wallet {
        #[command(subcommand)]
        command: WalletCommand,
    },
    /// Verified, bounded catalog or direct DHT results; not an exhaustive marketplace.
    Discover {
        #[arg(long)]
        model: Option<String>,
    },
    Quote {
        #[arg(long)]
        offer: PathBuf,
        #[arg(long)]
        amount_wei: String,
    },
    /// Prepare once, obtain a durable provider acknowledgement, then broadcast and open.
    Fund {
        #[arg(long)]
        offer: PathBuf,
        #[arg(long)]
        quote: PathBuf,
    },
    /// Resume a pending topup or opening with its original signed transaction (not cancellation).
    /// Refreshes existing accounts without rotating expired or revoked credentials.
    Resume {
        contract: String,
    },
    /// Explicit nonce recovery, NOT a refund. Costs gas; the original payment may win.
    /// Retries resubmit one fixed-fee self-transfer; never automatically bump fees.
    /// Defaults to a pending topup or unopened opening; otherwise requires --operation-id.
    CancelFunding {
        contract: String,
        #[arg(long)]
        operation_id: Option<String>,
        #[arg(long)]
        accept_gas_and_race_risk: bool,
    },
    /// Top up an existing immutable agreement; retries reuse the original transaction.
    Topup {
        contract: String,
        #[arg(long)]
        amount_wei: String,
    },
    Accounts,
    /// Show funding stages and transaction hashes, never signed transactions or credentials.
    Pending,
    Account {
        contract: String,
    },
    Usage {
        contract: String,
    },
    Funding {
        contract: String,
    },
    /// Reconcile wallet funding history, including cancelled topups recoverable after reorgs.
    /// Use an exact operation_id with cancel-funding to revisit historical or opening recovery.
    FundingRecovery {
        contract: String,
    },
    Receipt {
        contract: String,
    },
    Credentials {
        contract: String,
        #[command(subcommand)]
        command: CredentialCommand,
    },
    Infer {
        contract: String,
        #[arg(long)]
        request: PathBuf,
        #[arg(long)]
        stream: bool,
        #[arg(long, default_value = "chat")]
        kind: String,
    },
    Request {
        contract: String,
        request_id: String,
        #[arg(long)]
        cancel: bool,
    },
    /// Durable request keys for ambiguity lookup; contains no prompts or outputs.
    Requests {
        contract: String,
    },
    ProviderAdmin {
        #[arg(long)]
        endpoint: String,
        #[arg(long)]
        token_file: PathBuf,
        action: String,
    },
    Feedback {
        contract: String,
        #[arg(long)]
        score: u8,
        #[arg(long)]
        revision: u64,
    },
}
#[derive(Subcommand)]
enum NetworkCommand {
    Show,
    Status,
    Policy,
}
#[derive(Subcommand)]
enum WalletCommand {
    Create,
    /// Import a protected native key file; private key arguments/stdin are not accepted.
    Import {
        #[arg(long)]
        file: PathBuf,
    },
    /// Explicit protected backup; refuses to overwrite existing files.
    Export {
        #[arg(long)]
        file: PathBuf,
    },
    Address,
    Balance,
    History,
}
#[derive(Subcommand)]
enum CredentialCommand {
    List,
    /// Saves the new secret privately and prints metadata only.
    /// Activates an inference-capable key only when no valid native default exists.
    Issue {
        #[arg(long)]
        options: Option<PathBuf>,
    },
    Revoke {
        id: String,
    },
    Rotate {
        id: String,
        #[arg(long)]
        options: Option<PathBuf>,
    },
    /// Explicitly print the secret for SDK use. Do not paste this output into logs.
    Export {
        #[arg(long)]
        id: Option<String>,
    },
}
fn document<T: DeserializeOwned>(path: &PathBuf) -> Result<T, String> {
    serde_json::from_slice(&fs::read(path).map_err(|_| "cannot read JSON input file")?)
        .map_err(|_| "invalid JSON input document".into())
}
fn value<T: serde::Serialize>(value: T) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|e| e.to_string())
}
fn options(path: Option<PathBuf>) -> Result<Value, String> {
    match path {
        Some(path) => document(&path),
        None => Ok(
            json!({"expires_at":protocol::now()?+30*86400,"spend_cap_wei":null,"max_concurrency":1,"scopes":["infer","read"]}),
        ),
    }
}
#[tokio::main]
async fn main() {
    if let Err(error) = run(Cli::parse()).await {
        eprintln!("chiral: {error}");
        std::process::exit(1);
    }
}
async fn run(cli: Cli) -> Result<(), String> {
    let consumer = Consumer::load(&cli.config)?;
    let output = match cli.command {
        Command::Network { command } => match command {
            NetworkCommand::Show => value(consumer.network_config())?,
            NetworkCommand::Status => consumer.network_status().await?,
            NetworkCommand::Policy => consumer.version_status().await?,
        },
        Command::Wallet { command } => match command {
            WalletCommand::Create => json!({"address":consumer.wallet_create()?}),
            WalletCommand::Import { file } => {
                json!({"address":consumer.wallet_import_file(&file)?})
            }
            WalletCommand::Export { file } => {
                consumer.wallet_export_file(&file)?;
                json!({"exported":true})
            }
            WalletCommand::Address => json!({"address":consumer.address()?}),
            WalletCommand::Balance => consumer.balance().await?,
            WalletCommand::History => consumer.history().await?,
        },
        Command::Discover { model } => value(consumer.discover(model.as_deref()).await?)?,
        Command::Quote { offer, amount_wei } => value(
            consumer
                .quote(&document::<Signed<InferenceOffer>>(&offer)?, &amount_wei)
                .await?,
        )?,
        Command::Fund { offer, quote } => {
            eprintln!("Funding is provider-specific and non-refundable. The client broadcasts only after a durable provider acknowledgement, but the provider receives independently broadcastable signed bytes and could broadcast them before or without acknowledging; this ordering is not cryptographic escrow. An HTTP error, timeout or missing reply does not prove that no money moved: inspect pending and resume CONTRACT_ID instead of funding again. Explicit cancel-funding costs gas and can lose the race.");
            value(
                consumer
                    .fund(
                        &document::<Signed<InferenceOffer>>(&offer)?,
                        &document::<Signed<Quote>>(&quote)?,
                    )
                    .await?,
            )?
        }
        Command::Resume { contract } => value(consumer.resume(&contract).await?)?,
        Command::CancelFunding {
            contract,
            operation_id,
            accept_gas_and_race_risk,
        } => {
            if !accept_gas_and_race_risk {
                return Err("cancellation spends gas and the original payment may still win (non-refundable); pass --accept-gas-and-race-risk to authorize this explicit wallet action".into());
            }
            eprintln!("Authorizing one fixed-fee same-nonce self-transfer (original gas price + floor(price/8) + 1 wei). Gas is spent; the original may still win. Only status=cancelled confirms a canonical successful replacement at the eSpace safe frontier. Repeating this command resubmits the exact same bytes, never bumps again.");
            consumer
                .cancel_funding(&contract, operation_id.as_deref())
                .await?
        }
        Command::Topup {
            contract,
            amount_wei,
        } => value(consumer.topup(&contract, &amount_wei).await?)?,
        Command::Accounts => value(consumer.accounts()?)?,
        Command::Pending => consumer.pending_accounts()?,
        Command::Requests { contract } => consumer.local_requests(&contract)?,
        Command::Account { contract } => value(consumer.account(&contract).await?)?,
        Command::Usage { contract } => consumer.usage(&contract).await?,
        Command::Funding { contract } => consumer.funding(&contract).await?,
        Command::FundingRecovery { contract } => consumer.funding_recovery(&contract).await?,
        Command::Receipt { contract } => value(consumer.receipt(&contract).await?)?,
        Command::Credentials { contract, command } => match command {
            CredentialCommand::List => consumer.credentials(&contract).await?,
            CredentialCommand::Issue { options: path } => {
                consumer.credential_issue(&contract, options(path)?).await?
            }
            CredentialCommand::Revoke { id } => consumer.credential_revoke(&contract, &id).await?,
            CredentialCommand::Rotate { id, options: path } => {
                consumer
                    .credential_rotate(&contract, &id, options(path)?)
                    .await?
            }
            CredentialCommand::Export { id } => {
                consumer.credential_export(&contract, id.as_deref())?
            }
        },
        Command::Infer {
            contract,
            request,
            stream,
            kind,
        } => {
            let request = document::<Value>(&request)?;
            if stream {
                let mut response = consumer.stream_kind(&contract, &kind, request).await?;
                let mut failure = None;
                let mut completed = false;
                let mut cancelled = false;
                loop {
                    tokio::select! {
                        signal=tokio::signal::ctrl_c()=>{signal.map_err(|_|"cannot install cancellation signal")?;response.cancel.cancel();cancelled=true;eprintln!("Inference cancelled; inspect usage for final settlement.");break;},
                        event=response.events.recv()=>match event {
                            Some(event)=>{
                                if event.kind=="error" {failure=Some("stream failed; inspect usage before retrying".to_string());}
                                if event.kind=="done" {completed=true;}
                                println!("{}",serde_json::to_string(&event).map_err(|e|e.to_string())?);
                            },
                            None=>break,
                        }
                    }
                }
                if let Some(error) = failure {
                    return Err(error);
                }
                if !completed && !cancelled {
                    return Err(
                        "stream ended without completion; inspect usage before retrying".into(),
                    );
                }
                return Ok(());
            }
            consumer.infer(&contract, &kind, request).await?
        }
        Command::Request {
            contract,
            request_id,
            cancel,
        } => {
            if cancel {
                consumer.cancel_request(&contract, &request_id).await?
            } else {
                consumer.request_status(&contract, &request_id).await?
            }
        }
        Command::ProviderAdmin {
            endpoint,
            token_file,
            action,
        } => {
            consumer
                .provider_admin(&endpoint, &token_file, &action)
                .await?
        }
        Command::Feedback {
            contract,
            score,
            revision,
        } => consumer.feedback(&contract, score, revision).await?,
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&output).map_err(|e| e.to_string())?
    );
    Ok(())
}
