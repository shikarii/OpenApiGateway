use std::pin::Pin;
use std::sync::Arc;

use futures_util::{Stream, StreamExt};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

use crate::proto::envoy::service::ext_proc::v3::external_processor_server::{
    ExternalProcessor, ExternalProcessorServer,
};
use crate::proto::envoy::service::ext_proc::v3::{ProcessingRequest, ProcessingResponse};

use super::processor::ExtProcProcessor;

type ResponseStream =
    Pin<Box<dyn Stream<Item = Result<ProcessingResponse, Status>> + Send + 'static>>;

/// ext_proc gRPC server wrapper.
#[derive(Debug)]
pub(crate) struct ExtProcService {
    processor: Arc<ExtProcProcessor>,
}

impl ExtProcService {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            processor: Arc::new(ExtProcProcessor),
        })
    }

    pub fn server(self: Arc<Self>) -> ExternalProcessorServer<Self> {
        ExternalProcessorServer::new((*self).clone())
    }
}

impl Clone for ExtProcService {
    fn clone(&self) -> Self {
        Self {
            processor: Arc::clone(&self.processor),
        }
    }
}

#[tonic::async_trait]
impl ExternalProcessor for ExtProcService {
    type ProcessStream = ResponseStream;

    async fn process(
        &self,
        request: Request<tonic::Streaming<ProcessingRequest>>,
    ) -> Result<Response<Self::ProcessStream>, Status> {
        let mut inbound = request.into_inner();
        let processor = Arc::clone(&self.processor);
        let (tx, rx) = mpsc::channel(16);

        tokio::spawn(async move {
            while let Some(item) = inbound.next().await {
                match item {
                    Ok(message) => {
                        if tx.send(Ok(processor.process(message))).await.is_err() {
                            break;
                        }
                    }
                    Err(status) => {
                        let _ = tx.send(Err(status)).await;
                        break;
                    }
                }
            }
        });

        Ok(Response::new(
            Box::pin(ReceiverStream::new(rx)) as Self::ProcessStream
        ))
    }
}
