//! Minimal interoperable messages for SagerNet sing-box 1.14 StartedService.
use anyhow::{Context, Result};
use prost::Message;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tonic::{transport::Channel, Request};

#[derive(Clone, PartialEq, Message)]
pub struct Empty {}
#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct Version {
    #[prost(string, tag = "1")]
    pub version: String,
    #[prost(int32, tag = "2")]
    pub api_version: i32,
}
#[derive(Clone, PartialEq, Message)]
pub struct Interval {
    #[prost(int64, tag = "1")]
    pub interval: i64,
}
#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct Status {
    #[prost(uint64, tag = "1")]
    pub memory: u64,
    #[prost(int32, tag = "3")]
    pub connections_in: i32,
    #[prost(int32, tag = "4")]
    pub connections_out: i32,
    #[prost(bool, tag = "5")]
    pub traffic_available: bool,
    #[prost(int64, tag = "6")]
    pub uplink: i64,
    #[prost(int64, tag = "7")]
    pub downlink: i64,
    #[prost(int64, tag = "8")]
    pub uplink_total: i64,
    #[prost(int64, tag = "9")]
    pub downlink_total: i64,
}
#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct Groups {
    #[prost(message, repeated, tag = "1")]
    pub group: Vec<Group>,
}
#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct Group {
    #[prost(string, tag = "1")]
    pub tag: String,
    #[prost(string, tag = "2")]
    pub kind: String,
    #[prost(bool, tag = "3")]
    pub selectable: bool,
    #[prost(string, tag = "4")]
    pub selected: String,
    #[prost(message, repeated, tag = "6")]
    pub items: Vec<GroupItem>,
}
#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct GroupItem {
    #[prost(string, tag = "1")]
    pub tag: String,
    #[prost(string, tag = "2")]
    pub kind: String,
    #[prost(int64, tag = "3")]
    pub time: i64,
    #[prost(int32, tag = "4")]
    pub delay: i32,
}
#[derive(Clone, PartialEq, Message)]
pub struct Selection {
    #[prost(string, tag = "1")]
    pub group_tag: String,
    #[prost(string, tag = "2")]
    pub outbound_tag: String,
}
#[derive(Clone, PartialEq, Message)]
pub struct Test {
    #[prost(string, tag = "1")]
    pub outbound_tag: String,
}
#[derive(Clone, PartialEq, Message)]
pub struct Log {
    #[prost(message, repeated, tag = "1")]
    pub messages: Vec<LogMessage>,
}
#[derive(Clone, PartialEq, Message)]
pub struct LogMessage {
    #[prost(int32, tag = "1")]
    pub level: i32,
    #[prost(string, tag = "2")]
    pub message: String,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct Connection {
    #[prost(string, tag = "1")]
    pub id: String,
    #[prost(string, tag = "2")]
    pub inbound: String,
    #[prost(string, tag = "3")]
    pub inbound_type: String,
    #[prost(string, tag = "5")]
    pub network: String,
    #[prost(string, tag = "6")]
    pub source: String,
    #[prost(string, tag = "7")]
    pub destination: String,
    #[prost(string, tag = "8")]
    pub domain: String,
    #[prost(string, tag = "9")]
    pub protocol: String,
    #[prost(int64, tag = "12")]
    pub created_at: i64,
    #[prost(int64, tag = "13")]
    pub closed_at: i64,
    #[prost(int64, tag = "16")]
    pub uplink_total: i64,
    #[prost(int64, tag = "17")]
    pub downlink_total: i64,
    #[prost(string, tag = "18")]
    pub rule: String,
    #[prost(string, tag = "19")]
    pub outbound: String,
    #[prost(string, tag = "20")]
    pub outbound_type: String,
    #[prost(string, repeated, tag = "21")]
    pub chain: Vec<String>,
    #[prost(message, optional, tag = "22")]
    pub process: Option<ProcessInfo>,
}
#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct ProcessInfo {
    #[prost(uint32, tag = "1")]
    pub pid: u32,
    #[prost(string, tag = "4")]
    pub path: String,
}
#[derive(Clone, PartialEq, Message)]
pub struct ConnectionEvent {
    #[prost(int32, tag = "1")]
    pub kind: i32,
    #[prost(string, tag = "2")]
    pub id: String,
    #[prost(message, optional, tag = "3")]
    pub connection: Option<Connection>,
}
#[derive(Clone, PartialEq, Message)]
pub struct ConnectionEvents {
    #[prost(message, repeated, tag = "1")]
    pub events: Vec<ConnectionEvent>,
    #[prost(bool, tag = "2")]
    pub reset: bool,
}
#[derive(Clone, PartialEq, Message)]
struct CloseConnection {
    #[prost(string, tag = "1")]
    id: String,
}

pub struct Api {
    client: tonic::client::Grpc<Channel>,
    secret: String,
}
impl Api {
    pub async fn connections(&mut self) -> Result<Vec<Connection>> {
        let snapshot: ConnectionEvents = self
            .first(
                "/daemon.StartedService/SubscribeConnections",
                Interval {
                    interval: 1_000_000_000,
                },
            )
            .await?;
        anyhow::ensure!(
            snapshot.reset,
            "Core did not provide an initial connection snapshot"
        );
        Ok(snapshot
            .events
            .into_iter()
            .filter(|e| e.kind == 0)
            .filter_map(|e| e.connection)
            .collect())
    }
    pub async fn close_connection(&mut self, id: String) -> Result<()> {
        let _: Empty = self
            .unary(
                "/daemon.StartedService/CloseConnection",
                CloseConnection { id },
            )
            .await?;
        Ok(())
    }
    pub async fn logs(&mut self) -> Result<Log> {
        self.first("/daemon.StartedService/SubscribeLog", Empty {})
            .await
    }
    pub async fn connect(port: u16, secret: &str) -> Result<Self> {
        let channel = Channel::from_shared(format!("http://127.0.0.1:{port}"))?
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(3))
            .connect()
            .await?;
        Ok(Self {
            client: tonic::client::Grpc::new(channel),
            secret: secret.into(),
        })
    }
    fn request<T>(&self, body: T) -> Result<Request<T>> {
        let mut r = Request::new(body);
        r.metadata_mut()
            .insert("authorization", format!("Bearer {}", self.secret).parse()?);
        Ok(r)
    }
    async fn unary<
        Q: Message + Default + Send + Sync + 'static,
        R: Message + Default + Send + Sync + 'static,
    >(
        &mut self,
        path: &'static str,
        body: Q,
    ) -> Result<R> {
        self.client
            .ready()
            .await
            .context("gRPC service unavailable")?;
        let req = self.request(body)?;
        Ok(self
            .client
            .unary(
                req,
                tonic::codegen::http::uri::PathAndQuery::from_static(path),
                tonic::codec::ProstCodec::<Q, R>::default(),
            )
            .await?
            .into_inner())
    }
    async fn first<
        Q: Message + Default + Send + Sync + 'static,
        R: Message + Default + Send + Sync + 'static,
    >(
        &mut self,
        path: &'static str,
        body: Q,
    ) -> Result<R> {
        self.client
            .ready()
            .await
            .context("gRPC service unavailable")?;
        let req = self.request(body)?;
        let mut stream = self
            .client
            .server_streaming(
                req,
                tonic::codegen::http::uri::PathAndQuery::from_static(path),
                tonic::codec::ProstCodec::<Q, R>::default(),
            )
            .await?
            .into_inner();
        tokio::time::timeout(Duration::from_secs(3), stream.message())
            .await??
            .context("gRPC stream ended")
    }
    pub async fn version(&mut self) -> Result<Version> {
        self.unary("/daemon.StartedService/GetVersion", Empty {})
            .await
    }
    pub async fn status(&mut self) -> Result<Status> {
        self.first(
            "/daemon.StartedService/SubscribeStatus",
            Interval {
                interval: 1_000_000_000,
            },
        )
        .await
    }
    pub async fn groups(&mut self) -> Result<Groups> {
        self.first("/daemon.StartedService/SubscribeGroups", Empty {})
            .await
    }
    pub async fn select(&mut self, tag: String) -> Result<()> {
        self.select_group("proxy".into(), tag).await
    }
    pub async fn select_group(&mut self, group: String, tag: String) -> Result<()> {
        let _: Empty = self
            .unary(
                "/daemon.StartedService/SelectOutbound",
                Selection {
                    group_tag: group,
                    outbound_tag: tag,
                },
            )
            .await?;
        Ok(())
    }
    pub async fn select_confirmed(&mut self, group: String, tag: String) -> Result<Groups> {
        self.select_group(group.clone(), tag.clone()).await?;
        for attempt in 0..4 {
            let groups = self.groups().await?;
            if groups
                .group
                .iter()
                .any(|g| g.tag == group && g.selected == tag)
            {
                return Ok(groups);
            }
            if attempt < 3 {
                tokio::time::sleep(std::time::Duration::from_millis(75)).await;
            }
        }
        anyhow::bail!(
            "Selection was sent but not confirmed by the core. Refresh groups before retrying."
        )
    }
    pub async fn test(&mut self, tag: String) -> Result<()> {
        let _: Empty = self
            .unary("/daemon.StartedService/URLTest", Test { outbound_tag: tag })
            .await?;
        Ok(())
    }
}
