//! The single-application adapter for transport-owned listener Runnables.

use std::future::Future;
use std::sync::Arc;
use std::time::SystemTime;

use async_trait::async_trait;
use claviron::http::config::ListenerConfig;
use claviron::http::ext::{ConnCtx, RealIpExt, ReceivedAt, RequestArrival};
use claviron::http::trust::Trust;
use claviron::http::{RouteBody, RouteResponse};
use claviron::runtime::{
    AppState, Component, RunContext, RunnableInstance, registry,
};
use claviron::transport::listener::{Adapter, Listener};
use claviron::transport::owner::Owner as TransportOwner;
use http::Request;

use crate::routing::{AppRequest, Application, RequestBody};

/// Cold declarations. Transport owns every named serving generation.
pub struct SocketIngress {
    listeners: Vec<Arc<Listener<ApplicationListener>>>,
}

impl SocketIngress {
    pub fn new(configs: Vec<Arc<ListenerConfig>>) -> Self {
        Self {
            listeners: configs
                .into_iter()
                .map(|config| {
                    Arc::new(Listener::new(ApplicationListener {
                        config,
                        #[cfg(feature = "tls")]
                        tls: None,
                    }))
                })
                .collect(),
        }
    }

    /// Serve every TLS listener with this certificate, and the client
    /// certificate it requires, if any.
    #[cfg(feature = "tls")]
    pub fn tls(
        self,
        tls: Option<crate::Tls>,
    ) -> Self {
        Self {
            listeners: self
                .listeners
                .iter()
                .map(|listener| {
                    Arc::new(Listener::new(ApplicationListener {
                        config: listener.config.clone(),
                        tls: tls.clone(),
                    }))
                })
                .collect(),
        }
    }

    /// Require a client certificate on the listeners named here, each
    /// verified as its declaration says, and on no other listener, whatever
    /// [`Self::tls`] required. An empty selection changes nothing. Every name
    /// must be a TLS listener of this ingress with a certificate to serve.
    #[cfg(feature = "tls")]
    pub fn client_auth(
        self,
        selection: impl IntoIterator<
            Item = (
                claviron::http::listener::ListenerId,
                claviron::tls::auth::ClientAuth,
            ),
        >,
    ) -> Result<Self, String> {
        let mut selection: std::collections::HashMap<_, _> =
            selection.into_iter().collect();
        if selection.is_empty() {
            return Ok(self);
        }
        for id in selection.keys() {
            let listener = self
                .listeners
                .iter()
                .find(|listener| listener.config.id == *id);
            crate::options::selectable(
                id,
                listener.map(|listener| listener.config.as_ref()),
            )?;
            if listener.is_some_and(|listener| listener.tls.is_none()) {
                return Err(format!(
                    "client_auth names '{id}', which has no certificate to serve"
                ));
            }
        }
        Ok(Self {
            listeners: self
                .listeners
                .iter()
                .map(|listener| {
                    let tls = listener.tls.clone().map(|mut tls| {
                        tls.client_auth = selection.remove(&listener.config.id);
                        tls
                    });
                    Arc::new(Listener::new(ApplicationListener {
                        config: listener.config.clone(),
                        tls,
                    }))
                })
                .collect(),
        })
    }
}

/// What the master states once its workers booted: the sockets each of them
/// accepts on.
impl claviron::transport::listener::DeclaresListeners for SocketIngress {
    fn declared_listeners(&self) -> Vec<Arc<ListenerConfig>> {
        self.listeners.iter().map(|listener| listener.config.clone()).collect()
    }
}

impl Component<AppState> for SocketIngress {
    type Deps = (Arc<Application>, Option<Arc<Trust>>);

    fn name(&self) -> &str {
        "SocketIngress"
    }

    fn validate(
        &self,
        state: &AppState,
    ) -> registry::Result<()> {
        for listener in &self.listeners {
            #[cfg(unix)]
            claviron::transport::unix::prepare(state, &listener.config)?;
        }
        Ok(())
    }

    fn runnables(self: Arc<Self>) -> Vec<RunnableInstance<AppState>> {
        self.listeners.iter().map(Listener::instance).collect()
    }
}

struct ApplicationListener {
    config: Arc<ListenerConfig>,
    #[cfg(feature = "tls")]
    tls: Option<crate::Tls>,
}

#[async_trait]
impl Adapter for ApplicationListener {
    type Deps = (Arc<Application>, Option<Arc<Trust>>);

    fn config(&self) -> Arc<ListenerConfig> {
        self.config.clone()
    }

    async fn serve(
        self: Arc<Self>,
        (application, trust): Self::Deps,
        state: AppState,
        context: RunContext,
    ) -> registry::Result<()> {
        #[cfg(feature = "tls")]
        let tls = match &self.tls {
            Some(tls) => {
                Some(with_authority(tls, &self.config.id, &state).await?)
            }
            None => None,
        };
        let owner =
            Arc::new(Front { application, trust: trust.unwrap_or_default() });
        claviron::transport::single::serve(
            self.config.clone(),
            owner,
            #[cfg(feature = "tls")]
            tls.as_deref(),
            &state,
            &context,
        )
        .await
    }
}

/// `tls` with the store authority its client authentication selects, read
/// from this process's certificate store as the listener starts. A CA bundle,
/// or no client authentication, needs nothing from the store.
#[cfg(feature = "tls")]
async fn with_authority<'a>(
    tls: &'a crate::Tls,
    listener: &claviron::http::listener::ListenerId,
    state: &AppState,
) -> registry::Result<std::borrow::Cow<'a, crate::Tls>> {
    let Some(source) = tls
        .client_auth
        .as_ref()
        .and_then(claviron::tls::auth::ClientAuth::store_authority)
    else {
        return Ok(std::borrow::Cow::Borrowed(tls));
    };
    let store = state
        .registry_ref()
        .resolve::<claviron::tls::CertificateStore>()
        .ok_or_else(|| {
            registry::Error::msg(format!(
                "listener '{listener}': client_auth.authority selects an authority of the certificate store, and this process declares none"
            ))
        })?;
    let authority = store.trust(source).await.map_err(|error| {
        registry::Error::msg(format!(
            "listener '{listener}': client_auth.authority: {error}"
        ))
    })?;
    Ok(std::borrow::Cow::Owned(tls.clone().authority(Arc::new(authority))))
}

struct Front {
    application: Arc<Application>,
    trust: Arc<Trust>,
}

impl TransportOwner for Front {
    fn execution(&self) -> claviron::runtime::ExecutorSpec {
        self.application.execution()
    }

    fn call(
        &self,
        request: Request<RouteBody>,
        connection: Option<&Arc<ConnCtx>>,
        _arrival: RequestArrival,
    ) -> impl Future<Output = RouteResponse> + Send + use<> {
        self.application.call(prepare(request, connection, &self.trust))
    }
}

fn prepare(
    request: Request<RouteBody>,
    connection: Option<&Arc<ConnCtx>>,
    trust: &Trust,
) -> AppRequest {
    let peer = connection
        .filter(|connection| {
            connection.transport
                != claviron::http::listener::ListenerTransport::Unix
        })
        .map(|connection| connection.remote);
    let resolved = trust.resolve(peer, request.headers());
    let (mut parts, body) = request.into_parts();
    if let Some(host) = resolved.host.as_deref()
        && let Ok(value) = http::HeaderValue::from_str(host)
    {
        parts.headers.insert(http::header::HOST, value);
    }
    let mut request = Request::from_parts(parts, RequestBody::new(body));
    let extensions = request.extensions_mut();
    extensions.insert(ReceivedAt(SystemTime::now()));
    if let Some(client) = resolved.client {
        extensions.insert(RealIpExt(client.ip()));
    }
    if let Some(forwarded) = resolved.forwarded(peer) {
        extensions.insert(forwarded);
    }
    if let Some(connection) = connection {
        extensions.insert(connection.clone());
    }
    request
}

#[cfg(all(test, unix))]
#[path = "../tests/socket.rs"]
mod tests;
