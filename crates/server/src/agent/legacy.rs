//! The pre-rebrand `rudder.agent.v1` gRPC service.
//!
//! Agents built before the Dymini Panel rebrand dial method paths under the old
//! proto package. The message layout never changed, so every frame converts
//! with a prost round-trip and the same session core serves both packages.
//! Older agents stay connected — and keep receiving their binary update —
//! until they restart onto the rebranded build.

use std::pin::Pin;

use dymini_panel_proto::legacy::node_agent_server::NodeAgent as LegacyNodeAgent;
use dymini_panel_proto::legacy::{AgentMessage, Frame, ServerMessage};
use dymini_panel_proto::prost::Message;
use futures_util::{Stream, StreamExt};
use tonic::{Request, Response, Status, Streaming};

use super::service::AgentService;

/// Wire-convert one prost message into the wire-compatible type of the other
/// package. Both packages encode the same fields with the same numbers, so the
/// round-trip is total; a decode failure would only mean the protos drifted.
fn convert<T: Message, U: Message + Default>(value: T) -> Result<U, Status> {
    U::decode(value.encode_to_vec().as_slice())
        .map_err(|_| Status::internal("legacy protocol conversion failed"))
}

pub struct LegacyAgentService {
    inner: AgentService,
}

impl LegacyAgentService {
    pub fn new(inner: AgentService) -> Self {
        Self { inner }
    }
}

type LegacyServerStream = Pin<Box<dyn Stream<Item = Result<ServerMessage, Status>> + Send>>;
type LegacyFrameStream = Pin<Box<dyn Stream<Item = Result<Frame, Status>> + Send>>;

#[tonic::async_trait]
impl LegacyNodeAgent for LegacyAgentService {
    type SessionStream = LegacyServerStream;
    type ChannelStream = LegacyFrameStream;

    async fn session(
        &self,
        request: Request<Streaming<AgentMessage>>,
    ) -> Result<Response<Self::SessionStream>, Status> {
        let server_id = self.inner.authenticate(&request).await?;
        let mut incoming = request.into_inner();
        let hello = match incoming.next().await {
            Some(Ok(AgentMessage {
                body: Some(dymini_panel_proto::legacy::agent_message::Body::Hello(hello)),
            })) => convert(hello)?,
            _ => return Err(Status::invalid_argument("the first message must be Hello")),
        };
        let rest = incoming.map(|item| item.and_then(convert));
        let response = self
            .inner
            .run_session(server_id, hello, Box::pin(rest))
            .await?;
        let converted = response.into_inner().map(|item| item.and_then(convert));
        Ok(Response::new(Box::pin(converted)))
    }

    async fn channel(
        &self,
        request: Request<Streaming<Frame>>,
    ) -> Result<Response<Self::ChannelStream>, Status> {
        let server_id = self.inner.authenticate(&request).await?;
        let mut incoming = request.into_inner();
        let first = incoming
            .next()
            .await
            .ok_or_else(|| Status::invalid_argument("empty channel"))??;
        let first = convert(first)?;
        let rest = incoming.map(|item| item.and_then(convert));
        let response = self
            .inner
            .run_channel(server_id, first, Box::pin(rest))
            .await?;
        let converted = response.into_inner().map(|item| item.and_then(convert));
        Ok(Response::new(Box::pin(converted)))
    }
}
