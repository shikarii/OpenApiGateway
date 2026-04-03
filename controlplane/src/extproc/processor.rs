use crate::proto::envoy::service::ext_proc::v3::{
    processing_request::Request as ProcessingRequestKind,
    processing_response::Response as ProcessingResponseKind, BodyResponse, CommonResponse,
    HeaderMutation, HeaderValue, HeaderValueOption, HeadersResponse, ImmediateResponse,
    ProcessingRequest, ProcessingResponse,
};

/// Stateless ext_proc request processor.
#[derive(Debug, Default)]
pub(crate) struct ExtProcProcessor;

impl ExtProcProcessor {
    pub fn process(&self, request: ProcessingRequest) -> ProcessingResponse {
        match request.request {
            Some(ProcessingRequestKind::RequestHeaders(headers)) => {
                if let Some(status_header) = headers.headers.iter().find(|header| {
                    header
                        .key
                        .eq_ignore_ascii_case("x-ext-proc-immediate-status")
                }) {
                    let status = status_header.value.parse::<u32>().unwrap_or(403);
                    let body = headers
                        .headers
                        .iter()
                        .find(|header| header.key.eq_ignore_ascii_case("x-ext-proc-immediate-body"))
                        .map(|header| header.value.clone())
                        .unwrap_or_else(|| "blocked by ext_proc".to_owned());
                    return ProcessingResponse {
                        response: Some(ProcessingResponseKind::ImmediateResponse(
                            ImmediateResponse {
                                status,
                                body,
                                header_mutation: None,
                            },
                        )),
                    };
                }

                let mutation = headers
                    .headers
                    .iter()
                    .find(|header| header.key.eq_ignore_ascii_case("x-ext-proc-set-header"))
                    .and_then(|header| header.value.split_once('='))
                    .map(|(key, value)| HeaderMutation {
                        set_headers: vec![HeaderValueOption {
                            header: Some(HeaderValue {
                                key: key.trim().to_owned(),
                                value: value.trim().to_owned(),
                            }),
                        }],
                        remove_headers: Vec::new(),
                    });

                ProcessingResponse {
                    response: Some(ProcessingResponseKind::RequestHeaders(HeadersResponse {
                        response: Some(CommonResponse {
                            header_mutation: mutation,
                            immediate_response: None,
                            clear_route_cache: false,
                        }),
                    })),
                }
            }
            Some(ProcessingRequestKind::RequestBody(_)) => empty_body_response(true),
            Some(ProcessingRequestKind::ResponseHeaders(_)) => ProcessingResponse {
                response: Some(ProcessingResponseKind::ResponseHeaders(HeadersResponse {
                    response: Some(CommonResponse {
                        header_mutation: None,
                        immediate_response: None,
                        clear_route_cache: false,
                    }),
                })),
            },
            Some(ProcessingRequestKind::ResponseBody(_)) => empty_body_response(false),
            None => ProcessingResponse { response: None },
        }
    }
}

fn empty_body_response(is_request: bool) -> ProcessingResponse {
    let body = BodyResponse {
        response: Some(CommonResponse {
            header_mutation: None,
            immediate_response: None,
            clear_route_cache: false,
        }),
    };

    ProcessingResponse {
        response: Some(if is_request {
            ProcessingResponseKind::RequestBody(body)
        } else {
            ProcessingResponseKind::ResponseBody(body)
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::envoy::service::ext_proc::v3::{
        processing_response::Response as ResponseKind, RequestHeaders,
    };

    #[test]
    fn request_headers_can_trigger_immediate_response() {
        let processor = ExtProcProcessor;
        let response = processor.process(ProcessingRequest {
            request: Some(ProcessingRequestKind::RequestHeaders(RequestHeaders {
                headers: vec![HeaderValue {
                    key: "x-ext-proc-immediate-status".to_owned(),
                    value: "401".to_owned(),
                }],
                method: "GET".to_owned(),
                path: "/private".to_owned(),
            })),
        });

        match response.response {
            Some(ResponseKind::ImmediateResponse(reply)) => assert_eq!(reply.status, 401),
            other => panic!("unexpected response: {other:?}"),
        }
    }
}
