// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::{HashMap, VecDeque};
use std::fs::{create_dir_all, read, write};
use std::path::{Path, PathBuf};
use std::string::ToString;
use std::sync::Mutex;

use regex::bytes::Regex;
use serde::{Deserialize, Serialize};
use tauri::{Manager, State};

const SAVED_SERVERS_PATH: &str = "saved-servers.json";
const USER_SETTINGS_PATH: &str = "settings.json";
const I18N_GLOBAL_CONFIG_PATH: &str = "i18n.json";
const DEFAULT_LANGUAGE_ID: &str = "en-US";
const LANGUAGE_NAME_KEY: &str = "name";

struct GlobalState {
    settings_path: PathBuf,
    saved_servers_path: PathBuf,
    saved_servers: Mutex<VecDeque<SavedServer>>,
    languages: HashMap<String, Language>,
    settings: Mutex<Settings>
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
    language: String
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
                        language: DEFAULT_LANGUAGE_ID.to_string()
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

            app.manage(GlobalState {
                settings_path,
                saved_servers_path,
                saved_servers: Mutex::new(saved_servers),
                languages,
                settings: Mutex::new(settings),
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
            list_clients
        ])
        .run(tauri::generate_context!())
        .expect("Error while running Tauri application");
}
