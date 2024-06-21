// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::{HashMap, VecDeque};
use std::fs::{copy, create_dir_all, read, read_dir, write};
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::string::ToString;
use std::sync::Mutex;

use ini::Ini;
use regex::bytes::Regex;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use tauri::{Manager, State};
use tokio::spawn;
use tokio::task::{JoinHandle, spawn_blocking};

use crate::proxy::prepare_proxy;

mod proxy;

const SAVED_SERVERS_PATH: &str = "saved-servers.json";
const USER_SETTINGS_PATH: &str = "settings.json";
const I18N_GLOBAL_CONFIG_PATH: &str = "i18n.json";
const DEFAULT_LANGUAGE_ID: &str = "en-US";
const LANGUAGE_NAME_KEY: &str = "name";
const USER_OPTIONS_TEMPLATE_PATH: &str = "user-options-template.ini";
const CLIENT_CONFIG_PATH: &str = "ClientConfig.ini";
const USER_OPTIONS_PATH: &str = "UserOptions.ini";
const ACTIVE_CLIENT_EXECUTABLE: &str = "CloneWars.exe";

struct GlobalState {
    settings_path: PathBuf,
    saved_servers_path: PathBuf,
    saved_servers: Mutex<VecDeque<SavedServer>>,
    languages: HashMap<String, Language>,
    settings: Mutex<Settings>,
    active_client_path: PathBuf,
    user_options_template_path: PathBuf,
    proxy_process: tokio::sync::Mutex<Option<(JoinHandle<()>, JoinHandle<()>)>>
}

#[derive(Clone, Deserialize, Serialize)]
struct SavedServer {
    nickname: String,
    udp_endpoint: String,
    https_endpoint: String
}

#[derive(Deserialize, Serialize)]
struct Settings {
    clients: HashMap<String, PathBuf>,
    language: String,
    proxy_port: u16
}

type Language = HashMap<String, String>;

fn language<'a>(languages: &'a HashMap<String, Language>, language_id: &String) -> &'a Language {
    languages.get(language_id)
        .or(languages.get(DEFAULT_LANGUAGE_ID))
        .expect("Missing default language")
}

fn i18n_value_for_language_id_and_key(languages: &HashMap<String, Language>, language_id: &String, key: &String) -> String {
   i18n_value_for_language_and_key(language(languages, language_id), language_id, key)
}

fn i18n_value_for_language_and_key(language: &Language, language_id: &String, key: &String) -> String {
    (
        *language.get(key)
            .expect(&format!("Requested unknown key {key} for language {language_id}"))
    ).clone()
}

fn write_json_to_app_data<T: Serialize>(value: &T, path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        create_dir_all(parent).map_err(|err| err.to_string())?
    }

    write(
        path,
        serde_json::to_vec_pretty(&(*value))
            .map_err(|err| err.to_string())?
    ).map_err(|err| err.to_string())
}

fn save_server_list(saved_servers: &VecDeque<SavedServer>, path: &Path) -> Result<(), String> {
    write_json_to_app_data(saved_servers, path)
}

fn detect_client_version(client_bytes: &[u8]) -> Option<String> {
    let version_regex = Regex::new(r"\d\.\d{3}\.\d\.\d{6}").expect("Unable to compile regex");
    version_regex.find(&client_bytes).map_or(
        None,
        |mat| String::from_utf8(Vec::from(mat.as_bytes())).ok()
    )
}

fn remove_missing_clients(settings: &mut Settings, settings_path: &PathBuf) -> Result<(), String> {
    settings.clients.retain(|_, path| path.try_exists().unwrap_or_else(|_| true));
    write_json_to_app_data(&(*settings), settings_path)
}

fn list_files(root_dir: &Path, filter: impl Fn(&Path) -> bool) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    let mut directories = VecDeque::new();
    directories.push_back(root_dir.to_path_buf());

    while let Some(dir) = directories.pop_front() {
        if dir.is_dir() {
            let mut entries = read_dir(dir)?;
            while let Some(entry) = entries.next() {
                let path = entry?.path();
                if path.is_dir() {
                    directories.push_back(path);
                } else if filter(&path) {
                    files.push(path.strip_prefix(root_dir).unwrap().to_path_buf());
                }
            }
        }
    }

    Ok(files)
}

fn is_web_downloaded_pack(file_name: &String) -> bool {
    file_name.contains("W_") && file_name.ends_with(".pack")
}

fn should_copy(path: &Path) -> bool {
    if let Some(file_name) = path.file_name() {
        if let Ok(file_name_str) = file_name.to_os_string().into_string() {
            file_name_str != ACTIVE_CLIENT_EXECUTABLE
                && file_name_str != CLIENT_CONFIG_PATH
                && file_name_str != USER_OPTIONS_PATH
                && !is_web_downloaded_pack(&file_name_str)
        } else {
            true
        }
    } else {
        true
    }
}

fn prepare_client(proxy_port: u16, client_path: &PathBuf, client_parent: &PathBuf, state: &State<GlobalState>) -> Result<(), String> {
    create_dir_all(&state.active_client_path).map_err(|err| err.to_string())?;

    let active_client_executable_path = state.active_client_path.join(ACTIVE_CLIENT_EXECUTABLE);
    copy(client_path, active_client_executable_path).map_err(|err| err.to_string())?;

    let client_files_to_copy = list_files(client_parent, should_copy)
        .map_err(|err| err.to_string())?;
    for path in client_files_to_copy {
        let source = client_parent.join(&path);
        let destination = state.active_client_path.join(&path);
        create_dir_all(&destination.parent().expect("Active client path has no parent"))
            .map_err(|err| err.to_string())?;
        copy(source, destination).map_err(|err| err.to_string())?;
    }

    let user_options_path = state.active_client_path.join(USER_OPTIONS_PATH);
    if !user_options_path.exists() {
        copy(&state.user_options_template_path, user_options_path).map_err(|err| err.to_string())?;
    }

    let proxy_url = format!("http://127.0.0.1:{}", proxy_port);
    let proxy_assets_url = format!("{}/assets", proxy_url);
    let proxy_card_assets_url = format!("{}/card_games/", proxy_assets_url);
    let proxy_crash_url = format!("{}/crash?code=G", proxy_url);
    let mut client_config = Ini::new();
    client_config.with_section::<String>(None)
        .set("World", "");
    client_config.with_section(Some("Paths"))
        .set("PathScripts", "./Resources/Scripts/")
        .set("PathUiModules", "./UI/UiModules/");
    client_config.with_section(Some("Libraries"))
        .set("GraphicsDLL", "./GraphicsDriver.dll")
        .set("GraphicsDLLd", "./GraphicsDriver.dll")
        .set("GraphicsDllDataPath", "./");
    client_config.with_section(Some("AssetDelivery"))
        .set("IndirectEnabled", "1")
        .set("IndirectServerAddress", proxy_assets_url)
        .set("TcgServerAddress", proxy_card_assets_url);
    client_config.with_section(Some("LoadingScreen"))
        .set("LoadingScreenMusicId", "1144");
    client_config.with_section(Some("WebResources"))
        .set("GameCrashUrl", proxy_crash_url);
    let client_config_path = state.active_client_path.join(CLIENT_CONFIG_PATH);
    client_config.write_to_file(client_config_path).map_err(|err| err.to_string())?;

    Ok(())
}

#[tauri::command]
fn current_language_id(state: State<GlobalState>) -> String {
    state.settings.lock().expect("Unable to lock settings")
        .language
        .clone()
}

#[tauri::command]
fn all_language_ids_names(state: State<GlobalState>) -> Vec<(String, String)> {
    state.languages.iter().map(|(language_id, language)|
        (
            (*language_id).clone(),
            i18n_value_for_language_and_key(language, language_id, &LANGUAGE_NAME_KEY.to_string())
        )
    ).collect()
}

#[tauri::command]
fn set_language(new_language_id: String, state: State<GlobalState>) -> Result<(), String> {
    let mut settings = state.settings.lock().expect("Unable to lock settings");
    settings.language = new_language_id;
    write_json_to_app_data(&(*settings), &state.settings_path)
}

#[tauri::command]
fn i18n_value_for_key(key: String, state: State<GlobalState>) -> String {
    let language_id = &state.settings.lock().expect("Unable to lock settings")
        .language;
    i18n_value_for_language_id_and_key(&state.languages, language_id, &key)
}

#[tauri::command]
fn load_saved_servers(state: State<GlobalState>) -> VecDeque<SavedServer> {
    let saved_servers = state.inner().saved_servers.lock()
        .expect("Unable to lock saved servers");
    (*saved_servers).clone()
}

#[tauri::command]
fn set_saved_server_nickname(index: usize, nickname: String, state: State<GlobalState>) -> Result<(), String> {
    let mut saved_servers = state.inner().saved_servers.lock()
        .expect("Unable to lock saved servers");
    saved_servers[index].nickname = nickname;
    save_server_list(&saved_servers, &state.saved_servers_path)
}

#[tauri::command]
fn set_saved_server_udp_endpoint(index: usize, udp_endpoint: String, state: State<GlobalState>) -> Result<(), String> {
    let mut saved_servers = state.inner().saved_servers.lock()
        .expect("Unable to lock saved servers");
    saved_servers[index].udp_endpoint = udp_endpoint;
    save_server_list(&saved_servers, &state.saved_servers_path)
}

#[tauri::command]
fn set_saved_server_https_endpoint(index: usize, https_endpoint: String, state: State<GlobalState>) -> Result<(), String> {
    let mut saved_servers = state.inner().saved_servers.lock()
        .expect("Unable to lock saved servers");
    saved_servers[index].https_endpoint = https_endpoint;
    save_server_list(&saved_servers, &state.saved_servers_path)
}

#[tauri::command]
fn add_saved_server(saved_server: SavedServer, state: State<GlobalState>) -> Result<(), String> {
    let mut saved_servers = state.inner().saved_servers.lock()
        .expect("Unable to lock saved servers");
    saved_servers.push_front(saved_server);
    save_server_list(&saved_servers, &state.saved_servers_path)
}

#[tauri::command]
fn remove_saved_server(index: usize, state: State<GlobalState>) -> Result<(), String> {
    let mut saved_servers = state.inner().saved_servers.lock()
        .expect("Unable to lock saved servers");
    saved_servers.remove(index);
    save_server_list(&saved_servers, &state.saved_servers_path)
}

#[tauri::command]
fn reorder_saved_servers(old_index: usize, new_index: usize, state: State<GlobalState>) -> Result<(), String> {
    let mut saved_servers = state.inner().saved_servers.lock()
        .expect("Unable to lock saved servers");
    let saved_server = saved_servers.remove(old_index)
        .expect("Tried to reorder non-existent server");
    saved_servers.insert(new_index, saved_server);
    save_server_list(&saved_servers, &state.saved_servers_path)
}

#[tauri::command]
fn add_client(path: PathBuf, state: State<GlobalState>) -> Result<String, String> {
    let client_bytes = read(path.clone()).map_err(|err| err.to_string())?;
    detect_client_version(&client_bytes).map_or(
        Err("The selected file is not an original Clone Wars Adventures client from 2014 or earlier.".to_string()),
        |client_version| {
            path.parent().ok_or("Cannot select the root folder as a client")?;

            let mut settings = state.settings.lock().expect("Unable to lock settings");
            settings.clients.insert(client_version.clone(), path);
            write_json_to_app_data(&(*settings), &state.settings_path)?;
            Ok(client_version)
        }
    )
}

#[tauri::command]
fn list_clients(state: State<GlobalState>) -> Vec<(String, PathBuf)> {
    let settings = state.inner().settings.lock().expect("Unable to lock settings");
    settings.clients.clone().into_iter().collect()
}

#[tauri::command]
async fn start_client(index: usize, version: String, state: State<'_, GlobalState>) -> Result<(), String> {
    let (proxy_port, client_directory, udp_endpoint, https_endpoint) = {
        let settings = state.inner().settings.lock().expect("Unable to lock settings");

        let proxy_port = settings.proxy_port;
        let client_path = settings.clients.get(&version).ok_or("Requested client version that does not exist")?;
        let client_directory = client_path.parent().ok_or("Client has no parent directory")?.to_path_buf();
        prepare_client(proxy_port, client_path, &client_directory, &state)?;
        
        let saved_servers = state.inner().saved_servers.lock()
            .expect("Unable to lock saved servers");

        let udp_endpoint = saved_servers[index].udp_endpoint.clone();
        let https_endpoint = Url::parse(&saved_servers[index].https_endpoint)
            .map_err(|err| format!("bad HTTPS endpoint: {}", err))?;

        (proxy_port, client_directory, udp_endpoint, https_endpoint)
    };

    let mut proxy_process_lock = state.proxy_process.lock().await;
    if let Some((old_proxy_process, ref mut old_client_process)) = &mut *proxy_process_lock {
        if !old_client_process.is_finished() {
            return Err("Game is already running".to_string());
        }

        println!("Previous proxy stopping");
        old_proxy_process.abort();
    }

    let proxy_future = prepare_proxy(proxy_port, &client_directory, https_endpoint)
        .await
        .map_err(|err| err.to_string())?;

    let proxy_process = spawn(proxy_future);

    let active_client_path = state.active_client_path.clone();
    let active_client_executable_path = active_client_path.join(ACTIVE_CLIENT_EXECUTABLE);
    let client_process = spawn_blocking(move || {
        let command = Command::new(active_client_executable_path)
            .current_dir(active_client_path)
            .arg(format!("inifile={}", CLIENT_CONFIG_PATH))
            .arg("Guid=1")
            .arg(format!("Server={}", udp_endpoint))
            .arg("Ticket=p7w9dGPBPbbm9ZG")
            .arg("Internationalization:Locale=8")
            .arg("LoadingScreenId=-1")
            .arg("LiveGamer=1")
            .spawn();
        match command {
            Ok(process) => {
                let possible_output = process.wait_with_output();
                match possible_output {
                    Ok(output) => {
                        println!(
                            "Client finished with status code: {}\nstdout:\n{}\nstderr:\n{}",
                            output.status,
                            String::from_utf8_lossy(&output.stdout),
                            String::from_utf8_lossy(&output.stderr)
                        );
                    },
                    Err(err) => println!("Failed to wait for client to finish: {}", err)
                }
            },
            Err(err) => println!("Client failed to start: {}", err)
        }
    });

    *proxy_process_lock = Some((proxy_process, client_process));

    Ok(())
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let app_data_dir = app.path_resolver().app_data_dir()
                .expect("Unable to resolve app data directory");

            let saved_servers_path = app_data_dir.join(SAVED_SERVERS_PATH);
            let saved_servers: VecDeque<SavedServer> = match read(&saved_servers_path) {
                Ok(bytes) => serde_json::from_slice(&bytes).expect("Bad saved servers config file"),
                Err(err) => {
                    println!("Unable to read saved servers file: {}", err);
                    VecDeque::new()
                }
            };

            let settings_path = app_data_dir.join(USER_SETTINGS_PATH);
            let mut settings: Settings = match read(&settings_path) {
                Ok(bytes) => serde_json::from_slice(&bytes).expect("Bad saved servers config file"),
                Err(err) => {
                    println!("Unable to read settings file: {}", err);
                    Settings {
                        clients: HashMap::new(),
                        language: DEFAULT_LANGUAGE_ID.to_string(),
                        proxy_port: 4001,
                    }
                }
            };
            if let Err(err) = remove_missing_clients(&mut settings, &settings_path) {
                println!("Unable to save settings file after removing missing clients: {}", err);
            }

            let languages_path = app.path_resolver().resolve_resource(I18N_GLOBAL_CONFIG_PATH)
                .expect("Unable to resolve languages file");
            let languages: HashMap<String, Language> = serde_json::from_slice(
                &read(&languages_path).expect("Missing languages file")
            ).expect("Bad languages file");

            let active_client_path = app_data_dir.join("active_client/");
            let user_options_template_path = app.path_resolver().resolve_resource(USER_OPTIONS_TEMPLATE_PATH)
                .expect("Unable to resolve user options template file");

            app.manage(GlobalState {
                settings_path,
                saved_servers_path,
                saved_servers: Mutex::new(saved_servers),
                languages,
                settings: Mutex::new(settings),
                active_client_path,
                user_options_template_path,
                proxy_process: tokio::sync::Mutex::new(None),
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            current_language_id,
            all_language_ids_names,
            set_language,
            i18n_value_for_key,
            load_saved_servers,
            set_saved_server_nickname,
            set_saved_server_udp_endpoint,
            set_saved_server_https_endpoint,
            add_saved_server,
            remove_saved_server,
            reorder_saved_servers,
            add_client,
            list_clients,
            start_client
        ])
        .run(tauri::generate_context!())
        .expect("Error while running Tauri application");
}
