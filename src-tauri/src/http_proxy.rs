use std::collections::{HashMap, VecDeque};
use std::ffi::OsStr;
use std::future::Future;
use std::io::SeekFrom;
use std::path::{Component, PathBuf};
use std::sync::Arc;

use axum::{Router, serve};
use axum::extract::{Path, Request, State};
use axum::http::StatusCode;
use axum::routing::get;
use bytes::Bytes;
use miniz_oxide::deflate::compress_to_vec_zlib;
use reqwest::{Client, Url};
use tokio::{io, spawn};
use tokio::fs::{OpenOptions, read, read_dir};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::net::TcpListener;

const COMPRESSED_MAGIC: u32 = 0xa1b2c3d4;
const ZLIB_COMPRESSION_LEVEL: u8 = 6;
const CRC_EXTENSION_SEPARATOR: &str = "_";
const COMPRESSED_EXTENSION: &str = "z";

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

async fn build_asset_map(client_folder: &std::path::Path) -> io::Result<AssetMap> {
    let mut asset_map = HashMap::new();
    let mut tasks = Vec::new();

    for path in list_files(client_folder).await? {
        if let Some(extension) = path.extension() {
            if extension == "pack" {
                tasks.push(spawn(list_assets_in_pack(path)));
                continue;
            }
        }

        // Exclude extraneous files exactly named "manifest.txt" because we rename the
        // real manifests to "manifest.txt"
        if path.file_name().map(|file_name| file_name == "manifest.txt").unwrap_or(false) {
            continue;
        }

        let file_data = read(&path).await?;
        let crc = crc32fast::hash(&file_data);

        let is_manifest = path.file_name()
            .map(|file_name| file_name.to_os_string()
                .into_string()
                .ok()
                .map(|file_str| file_str.ends_with("_manifest.txt"))
                .unwrap_or(false)
            ).unwrap_or(false);
        let asset_name = if is_manifest {
            path.with_file_name("manifest.txt")
        } else {
            path.clone()
        };

        // Always overwrite in-pack assets with assets outside a pack
        asset_map.insert(asset_name.strip_prefix(client_folder).unwrap().to_path_buf(), AssetLocator {
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

fn decompose_extension(asset_name: &std::path::Path) -> (PathBuf, bool, Option<u32>) {
    let possible_extension_str = asset_name.extension()
        .map(|extension| extension.to_os_string().into_string().ok())
        .unwrap_or(None);
    let (non_crc_asset_name, crc) = if let Some(extension_str) = possible_extension_str {
        let extension_split = extension_str.rsplit_once(CRC_EXTENSION_SEPARATOR);

        if let Some((real_extension, crc_str)) = extension_split {
            (asset_name.with_extension(real_extension), crc_str.parse::<u32>().ok())
        } else {
            (asset_name.to_path_buf(), None)
        }
    } else {
        (asset_name.to_path_buf(), None)
    };

    let compressed = non_crc_asset_name.extension()
        .map(|extension| extension == COMPRESSED_EXTENSION)
        .unwrap_or(false);
    let uncompressed_asset_name = if compressed {
        non_crc_asset_name.with_extension("")
    } else {
        non_crc_asset_name.to_path_buf()
    };

    (
        uncompressed_asset_name,
        compressed,
        crc
    )
}

async fn build_local_asset_response(asset_locator: &AssetLocator, compress: bool) -> io::Result<Vec<u8>> {
    let mut buffer = Vec::new();

    // Read file from local client folder
    let mut file = OpenOptions::new()
        .read(true)
        .open(&asset_locator.path)
        .await?;
    file.seek(SeekFrom::Start(asset_locator.data_offset))
        .await?;

    let mut file_buffer = vec![0; asset_locator.size as usize];
    file.read_exact(&mut file_buffer).await?;

    if compress {
        buffer.write_u32(COMPRESSED_MAGIC).await?;
        buffer
            .write_u32(file_buffer.len() as u32)
            .await?;
        buffer.append(&mut compress_to_vec_zlib(
            &file_buffer,
            ZLIB_COMPRESSION_LEVEL,
        ));
    } else {
        buffer.append(&mut file_buffer);
    }

    Ok(buffer)
}

async fn retrieve_asset(
    asset_name: PathBuf,
    http_client: Arc<Client>,
    asset_map: Arc<AssetMap>,
    game_server_url: Arc<Url>,
    request: Request
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

    let (uncompressed_asset_name, compress, queried_crc) = decompose_extension(&asset_name);

    let possible_file_data = if let Some(asset_locator) = asset_map.get(&uncompressed_asset_name) {
        let crc = queried_crc.unwrap_or(asset_locator.crc);
        if crc == asset_locator.crc {
            build_local_asset_response(&asset_locator, compress).await.ok()
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

        let response = http_client.get(url)
            .send()
            .await
            .map_err(|err| err.status().unwrap_or(StatusCode::INTERNAL_SERVER_ERROR))?;

        match response.status() {
            StatusCode::OK => Ok(
                response
                    .bytes()
                    .await
                    .map_err(|err| err.status().unwrap_or(StatusCode::INTERNAL_SERVER_ERROR))?
            ),
            status_code => Err(status_code),
        }
    }
}

fn is_name_hash(component: &OsStr) -> bool {
    let is_hash_length = component.len() == 3;
    is_hash_length
        && if let Ok(comp_str) = component.to_os_string().into_string() {
        comp_str.parse::<u16>().is_ok()
    } else {
        false
    }
}

async fn asset_handler(
    Path(asset): Path<PathBuf>,
    State((http_client, asset_map, game_server_url)): State<(Arc<Client>, Arc<AssetMap>, Arc<Url>)>,
    request: Request
) -> Result<Bytes, StatusCode> {
    let is_first_component_name_hash = asset.iter().next().map(is_name_hash).unwrap_or(false);

    // Ignore the name hash if it is included
    let asset_name = if is_first_component_name_hash {
        let mut components = asset.components();
        components.next();
        components.as_path().to_path_buf()
    } else {
        asset
    };

    retrieve_asset(asset_name, http_client, asset_map, game_server_url, request).await
}

async fn start_proxy(listener: TcpListener, app: Router) {
    serve(listener, app).await.expect("Unable to start proxy");
}

pub async fn prepare_proxy(port: u16, client_folder: &std::path::Path, game_server_uri: Url) -> io::Result<impl Future<Output=()>> {
    let client = Client::new();
    let asset_map = build_asset_map(client_folder).await?;
    let app = Router::new()
        .route("/assets/*asset", get(asset_handler))
        .with_state((Arc::new(client), Arc::new(asset_map), Arc::new(game_server_uri.clone())));

    let listener = TcpListener::bind(format!("127.0.0.1:{}", port))
        .await?;
    println!("Proxy listening on {}", listener.local_addr().expect("Listener has no address"));
    Ok(start_proxy(listener, app))
}
