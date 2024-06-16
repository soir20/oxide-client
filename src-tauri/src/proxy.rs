use std::collections::{HashMap, VecDeque};
use std::io::SeekFrom;
use std::path::{Component, PathBuf};
use std::sync::Arc;

use axum::{Router, serve};
use axum::extract::{Path, Request, State};
use axum::http::StatusCode;
use axum::routing::get;
use reqwest::{Client, Url};
use tokio::fs::{OpenOptions, read, read_dir};
use tokio::{io, spawn};
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::net::TcpListener;
use bytes::Bytes;

async fn list_files(root_dir: &std::path::Path) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    let mut directories = VecDeque::new();
    directories.push_back(root_dir.to_path_buf());

    while let Some(dir) = directories.pop_front() {
        if dir.is_dir() {
            let mut entries = read_dir(dir).await?;
            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();
                if path.is_dir() {
                    directories.push_back(path);
                } else {
                    files.push(path);
                }
            }
        }
    }

    Ok(files)
}

struct Asset {
    name: PathBuf,
    data_offset: u64,
    size: u32,
    crc: u32,
}

struct AssetLocator {
    path: PathBuf,
    data_offset: u64,
    size: u32,
    crc: u32,
}

type AssetMap = HashMap<PathBuf, AssetLocator>;

async fn list_assets_in_pack(pack_path: PathBuf) -> io::Result<(PathBuf, Vec<Asset>)> {
    let mut file = OpenOptions::new()
        .read(true)
        .open(&pack_path)
        .await?;

    let mut results = Vec::new();
    loop {
        let next_group_offset = file.read_u32().await? as u64;
        let files_in_group = file.read_u32().await?;

        for _ in 0..files_in_group {
            let name_len = file.read_u32().await?;
            let mut name_buffer = vec![0; name_len as usize];
            file.read_exact(&mut name_buffer).await?;
            let name = PathBuf::from(
                String::from_utf8(name_buffer).map_err(|_| io::ErrorKind::InvalidData)?
            );

            let data_offset = file.read_u32().await? as u64;
            let size = file.read_u32().await?;
            let crc = file.read_u32().await?;

            results.push(Asset {
                name,
                data_offset,
                size,
                crc,
            });
        }

        if next_group_offset == 0 {
            break;
        }

        file.seek(SeekFrom::Start(next_group_offset)).await?;
    }

    Ok((pack_path, results))
}

async fn build_asset_map(client_folder: &PathBuf) -> io::Result<AssetMap> {
    let mut asset_map = HashMap::new();
    let mut tasks = Vec::new();

    for path in list_files(client_folder).await? {
        if let Some(extension) = path.extension() {
            if extension == "pack" {
                tasks.push(spawn(list_assets_in_pack(path)));
                continue;
            }
        }

        let file_data = read(&path).await?;
        let crc = crc32fast::hash(&file_data);

        // Always overwrite in-pack assets with assets outside a pack
        asset_map.insert(path.strip_prefix(client_folder).unwrap().to_path_buf(), AssetLocator {
            path,
            data_offset: 0,
            size: file_data.len() as u32,
            crc,
        });

    }

    for task in tasks {
        let (path, assets) = task.await??;
        for asset in assets {
            asset_map.entry(asset.name).or_insert(AssetLocator {
                path: path.clone(),
                data_offset: asset.data_offset,
                size: asset.size,
                crc: asset.crc,
            });
        }
    }

    Ok(asset_map)
}

async fn asset_handler(
    Path(asset_name): Path<PathBuf>,
    State((http_client, asset_map, game_server_url)): State<(Arc<Client>, Arc<AssetMap>, Arc<Url>)>,
    request: Request,
) -> Result<Bytes, StatusCode> {
    // SECURITY: Ensure that the path is within the assets cache before returning any data.
    // Reject all paths containing anything other than normal folder names (e.g. paths containing
    // the parent directory or the root directory).
    let is_invalid_path = asset_name
        .components()
        .any(|component| !matches!(component, Component::Normal(_)));
    if is_invalid_path {
        return Err(StatusCode::BAD_REQUEST);
    }

    let queried_crc = if let Some(query) = request.uri().query() {
        Some(str::parse(query).map_err(|_| StatusCode::BAD_REQUEST)?)
    } else {
        None
    };

    let possible_file_data = if let Some(asset_locator) = asset_map.get(&asset_name) {
        let crc = queried_crc.unwrap_or(asset_locator.crc);
        if crc == asset_locator.crc {
            read(&asset_locator.path)
                .await
                .ok()
        } else {
            None
        }
    } else {
        None
    };

    if let Some(file_data) = possible_file_data {
        Ok(file_data.into())
    } else {
        let request_path = request.uri().path();
        let path_and_query = request
            .uri()
            .path_and_query()
            .map(|path_and_query| path_and_query.as_str())
            .unwrap_or(request_path);
        let url = game_server_url.join(path_and_query)
            .map_err(|_| StatusCode::BAD_REQUEST)?;

        Ok(
            http_client.get(url)
                .send()
                .await
                .map_err(|err| err.status().unwrap_or(StatusCode::INTERNAL_SERVER_ERROR))?
                .bytes()
                .await
                .map_err(|err| err.status().unwrap_or(StatusCode::INTERNAL_SERVER_ERROR))?
        )
    }
}

pub async fn start_proxy(port: u16, client_folder: PathBuf, game_server_uri: Url) -> io::Result<()> {
    let client = Client::new();
    let asset_map = build_asset_map(&client_folder).await?;
    let app = Router::new()
        .route("/assets/*asset", get(asset_handler))
        .with_state((Arc::new(client), Arc::new(asset_map), Arc::new(game_server_uri.clone())));

    let listener = TcpListener::bind(format!("127.0.0.1:{}", port))
        .await?;
    println!("Proxy listening on {}", listener.local_addr().expect("Listener has no address"));
    serve(listener, app).await
}
