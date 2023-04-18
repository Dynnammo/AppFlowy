use std::time::Duration;
use std::{
  fmt,
  sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
  },
};

use tokio::sync::{broadcast, RwLock};

use flowy_client_ws::{listen_on_websocket, FlowyWebSocketConnect, NetworkType};
use flowy_database2::DatabaseManager2;
use flowy_document::entities::DocumentVersionPB;
use flowy_document::{DocumentConfig, DocumentManager};
use flowy_document2::manager::DocumentManager as DocumentManager2;
use flowy_error::FlowyResult;
use flowy_folder2::manager::Folder2Manager;
pub use flowy_net::get_client_server_configuration;
use flowy_net::local_server::LocalServer;
use flowy_net::ClientServerConfiguration;
use flowy_task::{TaskDispatcher, TaskRunner};
use flowy_user::event_map::UserStatusCallback;
use flowy_user::services::{UserSession, UserSessionConfig};
use lib_dispatch::prelude::*;
use lib_dispatch::runtime::tokio_default_runtime;
use lib_infra::future::{to_fut, Fut};
use module::make_plugins;
pub use module::*;
use user_model::UserProfile;

use crate::deps_resolve::*;

mod deps_resolve;
pub mod module;

static INIT_LOG: AtomicBool = AtomicBool::new(false);

/// This name will be used as to identify the current [AppFlowyCore] instance.
/// Don't change this.
pub const DEFAULT_NAME: &str = "appflowy";

#[derive(Clone)]
pub struct AppFlowyCoreConfig {
  /// Different `AppFlowyCoreConfig` instance should have different name
  name: String,
  /// Panics if the `root` path is not existing
  storage_path: String,
  log_filter: String,
  server_config: ClientServerConfiguration,
  pub document: DocumentConfig,
}

impl fmt::Debug for AppFlowyCoreConfig {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_struct("AppFlowyCoreConfig")
      .field("storage_path", &self.storage_path)
      .field("server-config", &self.server_config)
      .field("document-config", &self.document)
      .finish()
  }
}

impl AppFlowyCoreConfig {
  pub fn new(root: &str, name: String, server_config: ClientServerConfiguration) -> Self {
    AppFlowyCoreConfig {
      name,
      storage_path: root.to_owned(),
      log_filter: create_log_filter("info".to_owned(), vec![]),
      server_config,
      document: DocumentConfig::default(),
    }
  }

  pub fn with_document_version(mut self, version: DocumentVersionPB) -> Self {
    self.document.version = version;
    self
  }

  pub fn log_filter(mut self, level: &str, with_crates: Vec<String>) -> Self {
    self.log_filter = create_log_filter(level.to_owned(), with_crates);
    self
  }
}

fn create_log_filter(level: String, with_crates: Vec<String>) -> String {
  let level = std::env::var("RUST_LOG").unwrap_or(level);
  let mut filters = with_crates
    .into_iter()
    .map(|crate_name| format!("{}={}", crate_name, level))
    .collect::<Vec<String>>();
  filters.push(format!("flowy_core={}", level));
  filters.push(format!("flowy_folder={}", level));
  filters.push(format!("flowy_folder2={}", level));
  filters.push(format!("collab_folder={}", level));
  filters.push(format!("collab_persistence={}", level));
  filters.push(format!("collab_database={}", level));
  filters.push(format!("flowy_user={}", level));
  filters.push(format!("flowy_document={}", level));
  filters.push(format!("flowy_document2={}", level));
  filters.push(format!("flowy_database={}", level));
  filters.push(format!("flowy_database2={}", level));
  filters.push(format!("flowy_sync={}", "info"));
  filters.push(format!("flowy_client_sync={}", "info"));
  filters.push(format!("flowy_notification={}", "info"));
  filters.push(format!("lib_ot={}", level));
  filters.push(format!("lib_ws={}", level));
  filters.push(format!("lib_infra={}", level));
  filters.push(format!("flowy_sync={}", level));
  filters.push(format!("flowy_revision={}", level));
  filters.push(format!("flowy_revision_persistence={}", level));
  filters.push(format!("flowy_task={}", level));
  // filters.push(format!("lib_dispatch={}", level));

  filters.push(format!("dart_ffi={}", "info"));
  filters.push(format!("flowy_sqlite={}", "info"));
  filters.push(format!("flowy_net={}", level));
  #[cfg(feature = "profiling")]
  filters.push(format!("tokio={}", level));

  #[cfg(feature = "profiling")]
  filters.push(format!("runtime={}", level));

  filters.join(",")
}

#[derive(Clone)]
pub struct AppFlowyCore {
  #[allow(dead_code)]
  pub config: AppFlowyCoreConfig,
  pub user_session: Arc<UserSession>,
  pub document_manager: Arc<DocumentManager>,
  pub document_manager2: Arc<DocumentManager2>,
  pub folder_manager: Arc<Folder2Manager>,
  // pub database_manager: Arc<DatabaseManager>,
  pub database_manager: Arc<DatabaseManager2>,
  pub event_dispatcher: Arc<AFPluginDispatcher>,
  pub ws_conn: Arc<FlowyWebSocketConnect>,
  pub local_server: Option<Arc<LocalServer>>,
  pub task_dispatcher: Arc<RwLock<TaskDispatcher>>,
}

impl AppFlowyCore {
  pub fn new(config: AppFlowyCoreConfig) -> Self {
    #[cfg(feature = "profiling")]
    console_subscriber::init();

    init_log(&config);
    init_kv(&config.storage_path);
    tracing::debug!("🔥 {:?}", config);
    let runtime = tokio_default_runtime().unwrap();
    let task_scheduler = TaskDispatcher::new(Duration::from_secs(2));
    let task_dispatcher = Arc::new(RwLock::new(task_scheduler));
    runtime.spawn(TaskRunner::run(task_dispatcher.clone()));

    let (local_server, ws_conn) = mk_local_server(&config.server_config);
    let (
      user_session,
      document_manager,
      folder_manager,
      local_server,
      database_manager,
      document_manager2,
    ) = runtime.block_on(async {
      let user_session = mk_user_session(&config, &local_server, &config.server_config);
      let document_manager = DocumentDepsResolver::resolve(
        local_server.clone(),
        ws_conn.clone(),
        user_session.clone(),
        &config.server_config,
        &config.document,
      );
      let database_manager2 = Database2DepsResolver::resolve(
        ws_conn.clone(),
        user_session.clone(),
        task_dispatcher.clone(),
      )
      .await;

      let folder_manager =
        Folder2DepsResolver::resolve(user_session.clone(), &document_manager, &database_manager2)
          .await;

      let document_manager2 =
        Document2DepsResolver::resolve(user_session.clone(), &database_manager2);

      if let Some(local_server) = local_server.as_ref() {
        local_server.run();
      }
      ws_conn.init().await;
      (
        user_session,
        document_manager,
        folder_manager,
        local_server,
        database_manager2,
        document_manager2,
      )
    });

    let user_status_listener = UserStatusListener {
      document_manager: document_manager.clone(),
      folder_manager: folder_manager.clone(),
      database_manager: database_manager.clone(),
      ws_conn: ws_conn.clone(),
      config: config.clone(),
    };
    let user_status_callback = UserStatusCallbackImpl {
      listener: Arc::new(user_status_listener),
    };
    let cloned_user_session = user_session.clone();
    runtime.block_on(async move {
      cloned_user_session.clone().init(user_status_callback).await;
    });

    let event_dispatcher = Arc::new(AFPluginDispatcher::construct(runtime, || {
      make_plugins(
        &ws_conn,
        &folder_manager,
        &database_manager,
        &user_session,
        &document_manager,
        &document_manager2,
      )
    }));
    _start_listening(&event_dispatcher, &ws_conn, &folder_manager);

    Self {
      config,
      user_session,
      document_manager,
      document_manager2,
      folder_manager,
      database_manager,
      event_dispatcher,
      ws_conn,
      local_server,
      task_dispatcher,
    }
  }

  pub fn dispatcher(&self) -> Arc<AFPluginDispatcher> {
    self.event_dispatcher.clone()
  }
}

fn _start_listening(
  event_dispatcher: &AFPluginDispatcher,
  ws_conn: &Arc<FlowyWebSocketConnect>,
  folder_manager: &Arc<Folder2Manager>,
) {
  let subscribe_network_type = ws_conn.subscribe_network_ty();
  let folder_manager = folder_manager.clone();
  let _cloned_folder_manager = folder_manager;
  let ws_conn = ws_conn.clone();

  event_dispatcher.spawn(async move {
    listen_on_websocket(ws_conn.clone());
  });

  event_dispatcher.spawn(async move {
    _listen_network_status(subscribe_network_type).await;
  });
}

fn mk_local_server(
  server_config: &ClientServerConfiguration,
) -> (Option<Arc<LocalServer>>, Arc<FlowyWebSocketConnect>) {
  let ws_addr = server_config.ws_addr();
  if cfg!(feature = "http_sync") {
    let ws_conn = Arc::new(FlowyWebSocketConnect::new(ws_addr));
    (None, ws_conn)
  } else {
    let context = flowy_net::local_server::build_server(server_config);
    let local_ws = Arc::new(context.local_ws);
    let ws_conn = Arc::new(FlowyWebSocketConnect::from_local(ws_addr, local_ws));
    (Some(Arc::new(context.local_server)), ws_conn)
  }
}

async fn _listen_network_status(mut subscribe: broadcast::Receiver<NetworkType>) {
  while let Ok(_new_type) = subscribe.recv().await {
    // core.network_state_changed(new_type);
  }
}

fn init_kv(root: &str) {
  match flowy_sqlite::kv::KV::init(root) {
    Ok(_) => {},
    Err(e) => tracing::error!("Init kv store failed: {}", e),
  }
}

fn init_log(config: &AppFlowyCoreConfig) {
  if !INIT_LOG.load(Ordering::SeqCst) {
    INIT_LOG.store(true, Ordering::SeqCst);

    let _ = lib_log::Builder::new("AppFlowy-Client", &config.storage_path)
      .env_filter(&config.log_filter)
      .build();
  }
}

fn mk_user_session(
  config: &AppFlowyCoreConfig,
  local_server: &Option<Arc<LocalServer>>,
  server_config: &ClientServerConfiguration,
) -> Arc<UserSession> {
  let user_config = UserSessionConfig::new(&config.name, &config.storage_path);
  let cloud_service = UserDepsResolver::resolve(local_server, server_config);
  Arc::new(UserSession::new(user_config, cloud_service))
}

struct UserStatusListener {
  document_manager: Arc<DocumentManager>,
  folder_manager: Arc<Folder2Manager>,
  database_manager: Arc<DatabaseManager2>,
  ws_conn: Arc<FlowyWebSocketConnect>,
  #[allow(dead_code)]
  config: AppFlowyCoreConfig,
}

impl UserStatusListener {
  async fn did_sign_in(&self, token: &str, user_id: i64) -> FlowyResult<()> {
    self.folder_manager.initialize(user_id).await?;
    self.document_manager.initialize(user_id).await?;
    self.database_manager.initialize(user_id, token).await?;
    self
      .ws_conn
      .start(token.to_owned(), user_id.to_owned())
      .await?;
    Ok(())
  }

  async fn did_sign_up(&self, user_profile: &UserProfile) -> FlowyResult<()> {
    self
      .folder_manager
      .initialize_with_new_user(user_profile.id, &user_profile.token)
      .await?;
    self
      .document_manager
      .initialize_with_new_user(user_profile.id, &user_profile.token)
      .await?;

    self
      .database_manager
      .initialize_with_new_user(user_profile.id, &user_profile.token)
      .await?;

    self
      .ws_conn
      .start(user_profile.token.clone(), user_profile.id)
      .await?;
    Ok(())
  }

  async fn did_expired(&self, _token: &str, user_id: i64) -> FlowyResult<()> {
    self.folder_manager.clear(user_id).await;
    self.ws_conn.stop().await;
    Ok(())
  }
}

struct UserStatusCallbackImpl {
  listener: Arc<UserStatusListener>,
}

impl UserStatusCallback for UserStatusCallbackImpl {
  fn did_sign_in(&self, token: &str, user_id: i64) -> Fut<FlowyResult<()>> {
    let listener = self.listener.clone();
    let token = token.to_owned();
    let user_id = user_id.to_owned();
    to_fut(async move { listener.did_sign_in(&token, user_id).await })
  }

  fn did_sign_up(&self, user_profile: &UserProfile) -> Fut<FlowyResult<()>> {
    let listener = self.listener.clone();
    let user_profile = user_profile.clone();
    to_fut(async move { listener.did_sign_up(&user_profile).await })
  }

  fn did_expired(&self, token: &str, user_id: i64) -> Fut<FlowyResult<()>> {
    let listener = self.listener.clone();
    let token = token.to_owned();
    let user_id = user_id.to_owned();
    to_fut(async move { listener.did_expired(&token, user_id).await })
  }

  fn will_migrated(&self, _token: &str, _old_user_id: &str, _user_id: i64) -> Fut<FlowyResult<()>> {
    // Read the folder data
    todo!()
  }
}
