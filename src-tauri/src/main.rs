// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::VecDeque;
use std::fs::{create_dir_all, read, write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use serde::{Deserialize, Serialize};
use tauri::{Manager, State};

const SAVED_SERVERS_PATH: &str = "saved-servers.json";
const USER_SETTINGS_PATH: &str = "settings.json";
const I18N_GLOBAL_CONFIG_PATH: &str = "i18n.json";
const DEFAULT_LANGUAGE: &str = "en-US";

struct GlobalState {
    saved_servers_path: PathBuf,
    saved_servers: Mutex<VecDeque<SavedServer>>,
    settings: Mutex<Settings>
}

#[derive(Clone, Deserialize, Serialize)]
struct SavedServer {
    nickname: String,
    udp_endpoint: String,
    https_endpoint: String
}

struct Settings {
    language: String
}

#[tauri::command]
fn load_saved_servers(state: State<GlobalState>) -> VecDeque<SavedServer> {
    let saved_servers = state.inner().saved_servers.lock()
        .expect("Unable to lock saved servers");
    (*saved_servers).clone()
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

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let app_data_dir = app.path_resolver().app_data_dir().expect("Unable to resolve app data directory");

            let saved_servers_path = app_data_dir.join(SAVED_SERVERS_PATH);
            let saved_servers: VecDeque<SavedServer> = match read(&saved_servers_path) {
                Ok(bytes) => serde_json::from_slice(&bytes).expect("Bad saved servers config file"),
                Err(err) => {
                    println!("Unable to read saved servers file: {}", err);
                    VecDeque::new()
                }
            };

            app.manage(GlobalState {
                saved_servers_path,
                saved_servers: Mutex::new(saved_servers),
                settings: Mutex::new(Settings {
                    language: DEFAULT_LANGUAGE.to_string()
                }),
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            load_saved_servers,
            set_saved_server_nickname,
            set_saved_server_udp_endpoint,
            set_saved_server_https_endpoint,
            add_saved_server,
            remove_saved_server,
            reorder_saved_servers
        ])
        .run(tauri::generate_context!())
        .expect("Error while running Tauri application");
}
