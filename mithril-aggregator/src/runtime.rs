#![allow(dead_code, unused_imports)]
use super::dependency::{BeaconStoreWrapper, MultiSignerWrapper, SnapshotStoreWrapper};
use super::{BeaconStore, BeaconStoreError, ProtocolError, SnapshotError, Snapshotter};

use mithril_common::crypto_helper::Bytes;
use mithril_common::digesters::{Digester, DigesterError, ImmutableDigester};
use mithril_common::entities::{Beacon, Certificate, CertificatePending};
use mithril_common::fake_data;

use crate::dependency::{CertificatePendingStoreWrapper, CertificateStoreWrapper};
use crate::snapshot_stores::SnapshotStoreError;
use crate::snapshot_uploaders::{SnapshotLocation, SnapshotUploader};
use crate::DependencyManager;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use hex::ToHex;
use mithril_common::entities::Snapshot;
use slog_scope::{debug, error, info, trace, warn};
use std::fs::File;
use std::io;
use std::io::{Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;
use tokio::time::{sleep, Duration};

#[cfg(test)]
use mockall::automock;

#[derive(Error, Debug)]
pub enum RuntimeError {
    #[error("multi signer error")]
    MultiSigner(#[from] ProtocolError),

    #[error("beacon store error")]
    BeaconStore(#[from] BeaconStoreError),

    #[error("snapshotter error")]
    Snapshotter(#[from] SnapshotError),

    #[error("digester error")]
    Digester(#[from] DigesterError),

    #[error("snapshot store error")]
    SnapshotStore(#[from] SnapshotStoreError),

    #[error("certificate store error")]
    CertificateStore(String),

    #[error("snapshot uploader error: {0}")]
    SnapshotUploader(String),

    #[error("snapshot build error")]
    SnapshotBuild(#[from] io::Error),

    #[error("general error")]
    General(String),
}

pub struct IdleState {
    current_beacon: Option<Beacon>,
}

pub struct SigningState {
    current_beacon: Beacon,
    certificate_pending: CertificatePending,
}

pub enum AggregatorState {
    Idle(IdleState),
    Signing(SigningState),
}

pub struct AggregatorConfig {
    /// Interval between each snapshot, in seconds
    pub interval: u32,

    /// Cardano network
    pub network: String,

    /// DB directory to snapshot
    pub db_directory: PathBuf,

    /// Directory to store snapshot
    pub snapshot_directory: PathBuf,

    pub dependencies: Arc<DependencyManager>,
}

impl AggregatorConfig {
    pub fn new(
        interval: u32,
        network: &str,
        db_directory: &Path,
        snapshot_directory: &Path,
        dependencies: Arc<DependencyManager>,
    ) -> Self {
        Self {
            interval,
            network: network.to_string(),
            db_directory: db_directory.to_path_buf(),
            snapshot_directory: snapshot_directory.to_path_buf(),
            dependencies,
        }
    }
}

pub trait AggregatorRunnerTrait: Sync + Send {
    fn is_new_beacon(&self) -> bool;
}

pub struct AggregatorRunner {}

impl AggregatorRunner {
    pub fn new() -> Self {
        Self {}
    }
}

#[cfg_attr(test, automock)]
#[async_trait]
impl AggregatorRunnerTrait for AggregatorRunner {
    fn is_new_beacon(&self) -> bool {
        todo!()
    }
}

/// AggregatorRuntime
pub struct AggregatorRuntime {
    /// the internal state of the automate
    state: AggregatorState,

    /// configuration handler, also owns the dependencies
    config: AggregatorConfig,

    /// specific runner for this state machine
    runner: Arc<dyn AggregatorRunnerTrait>,
}

impl AggregatorRuntime {
    pub async fn new(
        config: AggregatorConfig,
        init_state: Option<AggregatorState>,
        runner: Arc<dyn AggregatorRunnerTrait>,
    ) -> Result<Self, RuntimeError> {
        info!("initializing runtime");

        let state = if init_state.is_none() {
            trace!("no initial state given");
            if config.dependencies.beacon_store.is_none() {
                trace!("idle state, no current beacon");
                AggregatorState::Idle(IdleState {
                    current_beacon: None,
                })
            } else {
                let store = config.dependencies.beacon_store.as_ref().unwrap();
                let current_beacon = store
                    .read()
                    .await
                    .get_current_beacon()
                    .await
                    .map_err(|e| RuntimeError::General(e.to_string()))?;
                trace!("idle state, got current beacon from store");

                AggregatorState::Idle(IdleState { current_beacon })
            }
        } else {
            trace!("got initial state from caller");
            init_state.unwrap()
        };

        Ok::<Self, RuntimeError>(Self {
            config,
            state,
            runner,
        })
    }

    pub async fn run(&self) {
        info!("Starting runtime");
        loop {
            if let Err(e) = self.cycle().await {
                error!("{:?}", e)
            }

            info!("Sleeping for {}", self.config.interval);
            sleep(Duration::from_millis(self.config.interval.into())).await;
        }
    }

    async fn cycle(&self) -> Result<(), RuntimeError> {
        Ok(())
    }
}
/// AggregatorRuntime factory
// TODO: Fix this by implementing an Aggregator Config that implements the From trait for a general Config
/*
       pub fn new(
           interval: u32,
           network: String,
           db_directory: PathBuf,
           snapshot_directory: PathBuf,
           beacon_store: BeaconStoreWrapper,
           multi_signer: MultiSignerWrapper,
           snapshot_store: SnapshotStoreWrapper,
           snapshot_uploader: Box<dyn SnapshotUploader>,
           certificate_pending_store: CertificatePendingStoreWrapper,
           certificate_store: CertificateStoreWrapper,
       ) -> Self {
           Self {
               interval,
               network,
               db_directory,
               snapshot_directory,
               beacon_store,
               multi_signer,
               snapshot_store,
               snapshot_uploader,
               certificate_pending_store,
               certificate_store,
           }
       }
    /// Run snapshotter loop
    pub async fn run(&self) {
        info!("Starting runtime");

        loop {
            if let Err(e) = self.do_work().await {
                error!("{:?}", e)
            }

            info!("Sleeping for {}", self.interval);
            sleep(Duration::from_millis(self.interval.into())).await;
        }
    }

    async fn do_work(&self) -> Result<(), RuntimeError> {
        let snapshotter =
            Snapshotter::new(self.db_directory.clone(), self.snapshot_directory.clone());
        let digester = ImmutableDigester::new(self.db_directory.clone(), slog_scope::logger());

        info!("Computing digest"; "db_directory" => self.db_directory.display());
        let digest_result = tokio::task::spawn_blocking(move || digester.compute_digest())
            .await
            .map_err(|e| RuntimeError::General(e.to_string()))?;
        match digest_result {
            Ok(digest_result) => {
                let mut beacon = fake_data::beacon();
                beacon.immutable_file_number = digest_result.last_immutable_file_number;
                let message = &digest_result.digest.clone().into_bytes();

                match self.manage_trigger_snapshot(message, &beacon).await {
                    Ok(Some(certificate)) => {
                        info!(
                            "Snapshotting immutables up to `{}` in an archive",
                            &beacon.immutable_file_number
                        );

                        let snapshot_name =
                            format!("{}.{}.tar.gz", self.network, &digest_result.digest);

                        let snapshot_path = tokio::task::spawn_blocking(
                            move || -> Result<PathBuf, SnapshotError> {
                                snapshotter.snapshot(&snapshot_name)
                            },
                        )
                        .await
                        .map_err(|e| RuntimeError::General(e.to_string()))??;

                        info!("Uploading snapshot archive");
                        let uploaded_snapshot_location = self
                            .snapshot_uploader
                            .upload_snapshot(&snapshot_path)
                            .await
                            .map_err(RuntimeError::SnapshotUploader)?;

                        info!(
                            "Snapshot archive uploaded, location: `{}`",
                            &uploaded_snapshot_location
                        );

                        let new_snapshot = build_new_snapshot(
                            digest_result.digest,
                            certificate.hash.to_owned(),
                            &snapshot_path,
                            uploaded_snapshot_location,
                        )?;

                        info!("Storing snapshot data"; "snapshot" => format!("{:?}", new_snapshot));
                        let mut snapshot_store = self.snapshot_store.write().await;
                        snapshot_store.add_snapshot(new_snapshot).await?;

                        info!("Storing certificate data"; "certificate" => format!("{:?}", certificate));
                        let mut certificate_store = self.certificate_store.write().await;
                        certificate_store
                            .save(certificate)
                            .await
                            .map_err(|e| RuntimeError::CertificateStore(e.to_string()))?;

                        Ok(())
                    }
                    Ok(None) => Ok(()),
                    Err(err) => Err(err),
                }
            }
            Err(err) => {
                let mut beacon_store = self.beacon_store.write().await;
                beacon_store.reset_current_beacon().await?;
                Err(RuntimeError::Digester(err))
            }
        }
    }

    async fn manage_trigger_snapshot(
        &self,
        message: &Bytes,
        beacon: &Beacon,
    ) -> Result<Option<Certificate>, RuntimeError> {
        let mut multi_signer = self.multi_signer.write().await;
        match multi_signer.get_multi_signature().await {
            Ok(None) => {
                {
                    let mut beacon_store = self.beacon_store.write().await;
                    beacon_store.set_current_beacon(beacon.clone()).await?;
                }
                multi_signer
                    .update_current_message(message.to_owned())
                    .await?;
                match multi_signer.create_multi_signature().await {
                    Ok(Some(_)) => {
                        let message = multi_signer
                            .get_current_message()
                            .await
                            .unwrap()
                            .encode_hex::<String>();
                        debug!(
                            "A multi signature has been created for message: {}",
                            message
                        );
                        let previous_hash = "".to_string();
                        Ok(multi_signer
                            .create_certificate(beacon.clone(), previous_hash)
                            .await?)
                    }
                    Ok(None) => {
                        warn!("Not ready to create a multi signature: quorum is not reached yet");
                        Ok(None)
                    }
                    Err(e) => {
                        warn!("Error while creating a multi signature: {}", e);
                        Err(RuntimeError::MultiSigner(e))
                    }
                }
            }
            Ok(_) => {
                let mut beacon_store = self.beacon_store.write().await;
                beacon_store.reset_current_beacon().await?;
                Ok(None)
            }
            Err(err) => {
                let mut beacon_store = self.beacon_store.write().await;
                beacon_store.reset_current_beacon().await?;
                Err(RuntimeError::MultiSigner(err))
            }
        }
    }
}
    */

fn build_new_snapshot(
    digest: String,
    certificate_hash: String,
    snapshot_filepath: &Path,
    uploaded_snapshot_location: SnapshotLocation,
) -> Result<Snapshot, RuntimeError> {
    let timestamp: DateTime<Utc> = Utc::now();
    let created_at = format!("{:?}", timestamp);

    let mut tar_gz = File::open(&snapshot_filepath)?;
    let size: u64 = tar_gz.seek(SeekFrom::End(0))?;

    Ok(Snapshot::new(
        digest,
        certificate_hash,
        size,
        created_at,
        vec![uploaded_snapshot_location],
    ))
}

#[cfg(test)]
mod tests {
    use super::super::Config;
    use super::*;

    async fn init_runtime(
        init_state: Option<AggregatorState>,
        runner: MockAggregatorRunner,
    ) -> AggregatorRuntime {
        let config: Config = config::Config::builder()
            .build()
            .map_err(|e| format!("configuration build error: {}", e))
            .unwrap()
            .try_deserialize()
            .map_err(|e| format!("configuration deserialize error: {}", e))
            .unwrap();
        let dependencies = Arc::new(DependencyManager::new(config));
        let config = AggregatorConfig::new(
            100,
            "test",
            Path::new("whatever"),
            Path::new("whatever"),
            dependencies,
        );

        AggregatorRuntime::new(config, init_state, Arc::new(runner))
            .await
            .unwrap()
    }
}
