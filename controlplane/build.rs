fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=../proto");

    tonic_build::configure()
        .build_server(true)
        .build_client(false)
        .compile_protos(
            &[
                "../proto/envoy/config/core/v3/base.proto",
                "../proto/envoy/config/endpoint/v3/endpoint.proto",
                "../proto/envoy/config/cluster/v3/cluster.proto",
                "../proto/envoy/config/route/v3/route.proto",
                "../proto/envoy/config/listener/v3/listener.proto",
                "../proto/envoy/extensions/filters/network/http_connection_manager/v3/http_connection_manager.proto",
                "../proto/envoy/extensions/filters/http/ext_authz/v3/ext_authz.proto",
                "../proto/envoy/extensions/filters/http/ext_proc/v3/ext_proc.proto",
                "../proto/envoy/extensions/filters/http/router/v3/router.proto",
                "../proto/envoy/service/discovery/v3/discovery.proto",
                "../proto/envoy/service/discovery/v3/ads.proto",
                "../proto/envoy/service/ext_proc/v3/external_processor.proto",
            ],
            &["../proto"],
        )?;

    Ok(())
}
