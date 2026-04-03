pub(crate) mod envoy {
    pub(crate) mod config {
        pub(crate) mod cluster {
            pub(crate) mod v3 {
                tonic::include_proto!("envoy.config.cluster.v3");
            }
        }
        pub(crate) mod core {
            pub(crate) mod v3 {
                tonic::include_proto!("envoy.config.core.v3");
            }
        }
        pub(crate) mod endpoint {
            pub(crate) mod v3 {
                tonic::include_proto!("envoy.config.endpoint.v3");
            }
        }
        pub(crate) mod listener {
            pub(crate) mod v3 {
                tonic::include_proto!("envoy.config.listener.v3");
            }
        }
        pub(crate) mod route {
            pub(crate) mod v3 {
                tonic::include_proto!("envoy.config.route.v3");
            }
        }
    }

    pub(crate) mod extensions {
        pub(crate) mod filters {
            pub(crate) mod http {
                pub(crate) mod ext_authz {
                    pub(crate) mod v3 {
                        tonic::include_proto!("envoy.extensions.filters.http.ext_authz.v3");
                    }
                }
                pub(crate) mod ext_proc {
                    pub(crate) mod v3 {
                        tonic::include_proto!("envoy.extensions.filters.http.ext_proc.v3");
                    }
                }
                pub(crate) mod router {
                    pub(crate) mod v3 {
                        tonic::include_proto!("envoy.extensions.filters.http.router.v3");
                    }
                }
            }
            pub(crate) mod network {
                pub(crate) mod http_connection_manager {
                    pub(crate) mod v3 {
                        tonic::include_proto!(
                            "envoy.extensions.filters.network.http_connection_manager.v3"
                        );
                    }
                }
            }
        }
    }

    pub(crate) mod service {
        pub(crate) mod discovery {
            pub(crate) mod v3 {
                tonic::include_proto!("envoy.service.discovery.v3");
            }
        }
        pub(crate) mod ext_proc {
            pub(crate) mod v3 {
                // Reason: prost-generated variants must mirror the upstream Envoy proto names.
                #![allow(clippy::enum_variant_names)]
                tonic::include_proto!("envoy.service.ext_proc.v3");
            }
        }
    }
}
