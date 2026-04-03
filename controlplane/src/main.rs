mod admin;
mod auth;
mod config;
mod extauthz;
mod extproc;
mod observability;
mod plugins;
mod proto;
mod ratelimit;
mod startup;
mod xds;

#[tokio::main]
async fn main() {
    startup::run().await;
}
