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

/// A selection asks for a client certificate on the listeners it names and on
/// no other, and names only TLS listeners that are declared.
#[cfg(feature = "tls")]
#[test]
fn client_auth_requires_a_certificate_only_on_the_listeners_it_names() {
    let mut options: crate::RunnerOptions = serde_yaml::from_str(
        "listeners:
  open: {transport: tcp, listen: 127.0.0.1:8001, protocols: [h1], tls: true}
  mutual: {transport: tcp, listen: 127.0.0.1:8002, protocols: [h2, h1], tls: true}
  plain: {transport: tcp, listen: 127.0.0.1:8003, protocols: [h1]}
tls_cert: server.pem
tls_key: server.key
client_auth:
  mutual: {authority: {local: example}}
",
    )
    .unwrap();
    options.validate().unwrap();
    let ingress = SocketIngress::new(options.socket_listeners().unwrap())
        .tls(options.tls())
        .client_auth(options.client_auth().unwrap())
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

    for (name, refusal) in [
        ("plain", "client_auth names 'plain', which does not serve TLS"),
        (
            "absent",
            "client_auth names 'absent', which is not a declared listener",
        ),
    ] {
        options.client_auth = [(
            name.parse().unwrap(),
            serde_yaml::from_str("ca_bundle: ca.pem").unwrap(),
        )]
        .into();
        assert!(options.validate().unwrap_err().starts_with(refusal));
    }
}
