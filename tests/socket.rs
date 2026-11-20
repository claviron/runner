use super::*;

#[test]
fn trusted_forwarding_prepares_client_identity_without_a_network_peer() {
    let trust = Trust::default();
    let request = Request::builder()
        .header("x-forwarded-for", "192.0.2.4")
        .body(claviron::http::empty_body())
        .unwrap();
    let request = prepare(request, None, &trust);
    assert!(request.extensions().get::<ReceivedAt>().is_some());
    assert_eq!(
        request.extensions().get::<RealIpExt>().unwrap().0,
        "192.0.2.4".parse::<std::net::IpAddr>().unwrap()
    );
    assert!(request.extensions().get::<Arc<ConnCtx>>().is_none());
}

/// A listener that declares a client policy requires a certificate, and no
/// other listener does; the process-wide client CA beside it is refused.
#[cfg(feature = "tls")]
#[test]
fn a_listener_requires_the_client_certificate_its_own_tls_declares() {
    let mut options: crate::RunnerOptions = serde_yaml::from_str(
        "listeners:
  open: {transport: tcp, listen: 127.0.0.1:8001, protocols: [h1], tls: on}
  mutual: {transport: tcp, listen: 127.0.0.1:8002, protocols: [h2, h1], tls: {client_auth: {authority: {local: example}}}}
  plain: {transport: tcp, listen: 127.0.0.1:8003, protocols: [h1]}
tls_cert: server.pem
tls_key: server.key
",
    )
    .unwrap();
    options.validate().unwrap();
    let ingress = SocketIngress::new(options.socket_listeners().unwrap())
        .tls(options.tls())
        .unwrap();
    let required: Vec<_> = ingress
        .listeners
        .iter()
        .filter(|listener| {
            listener.tls.as_ref().is_some_and(|tls| tls.client_auth.is_some())
        })
        .map(|listener| listener.config.id.as_str())
        .collect();
    assert_eq!(required, ["mutual"]);

    options.tls_client_ca = Some("ca.pem".into());
    assert!(
        options
            .validate()
            .unwrap_err()
            .starts_with("listener 'mutual': declare client certificates once")
    );
}
