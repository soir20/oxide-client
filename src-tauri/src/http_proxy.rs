use std::collections::{HashMap, VecDeque};
use std::ffi::OsStr;
use std::future::Future;
use std::io::{ErrorKind, SeekFrom};
use std::path::{Component, PathBuf};
use std::sync::Arc;

use axum::extract::{Path, Request, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{serve, Router};
use bytes::Bytes;
use miniz_oxide::deflate::compress_to_vec_zlib;
use miniz_oxide::inflate::{decompress_to_vec_zlib, DecompressError, TINFLStatus};
use reqwest::{Client, Url};
use tokio::fs::{read, read_dir, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::{io, spawn};

const COMPRESSED_MAGIC: u32 = 0xa1b2c3d4;
const ZLIB_COMPRESSION_LEVEL: u8 = 6;
const CRC_EXTENSION_SEPARATOR: &str = "_";
const COMPRESSED_EXTENSION: &str = "z";
const MANIFEST_CRC_FILE_NAME: &str = "manifest.crc";
const MANIFEST_FILE_NAME: &str = "manifest.txt";
const COMPRESSED_MANIFEST_FILE_NAME: &str = "manifest.txt.z";
const MANIFEST_SUFFIX: &str = "_manifest.txt";

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
    crc: u32,
    kind: AssetLocatorKind,
}

enum AssetLocatorKind {
    Memory(MemoryAssetLocator),
    File(FileAssetLocator),
}

struct MemoryAssetLocator {
    data: Vec<u8>,
}

struct FileAssetLocator {
    path: PathBuf,
    data_offset: u64,
    size: u32,
}

type AssetMap = HashMap<PathBuf, AssetLocator>;

async fn list_assets_in_pack(pack_path: PathBuf) -> io::Result<(PathBuf, Vec<Asset>)> {
    let mut file = OpenOptions::new().read(true).open(&pack_path).await?;

    let mut results = Vec::new();
    loop {
        let next_group_offset = file.read_u32().await? as u64;
        let files_in_group = file.read_u32().await?;

        for _ in 0..files_in_group {
            let name_len = file.read_u32().await?;
            let mut name_buffer = vec![0; name_len as usize];
            file.read_exact(&mut name_buffer).await?;
            let name =
                PathBuf::from(String::from_utf8(name_buffer).map_err(|_| ErrorKind::InvalidData)?);

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

fn file_name_ends_with(path: &std::path::Path, suffix: &str) -> bool {
    path.file_name()
        .map(|file_name| {
            file_name
                .to_os_string()
                .into_string()
                .ok()
                .map(|file_str| file_str.ends_with(suffix))
                .unwrap_or(false)
        })
        .unwrap_or(false)
}

fn decompress_asset_response(file_data: Vec<u8>) -> Result<Vec<u8>, DecompressError> {
    if file_data.len() > 8 {
        // Skip the 4-byte magic number and 4-byte length comprising the compressed header
        decompress_to_vec_zlib(&file_data[8..])
    } else {
        Err(DecompressError {
            status: TINFLStatus::NeedsMoreInput,
            output: file_data,
        })
    }
}

async fn build_asset_map(
    client_folder: &std::path::Path,
    http_client: &Arc<Client>,
    game_server_url: &Arc<Url>,
) -> io::Result<AssetMap> {
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
        if path
            .file_name()
            .map(|file_name| file_name == MANIFEST_FILE_NAME)
            .unwrap_or(false)
        {
            continue;
        }

        let mut file_data = read(&path).await?;

        let path_without_prefix = path.strip_prefix(client_folder).unwrap().to_path_buf();
        if file_name_ends_with(&path_without_prefix, MANIFEST_SUFFIX) {
            let compressed_manifest_path =
                path_without_prefix.with_file_name(COMPRESSED_MANIFEST_FILE_NAME);

            let mut remote_manifest = if let Some(manifest_path_str) =
                compressed_manifest_path.to_str()
            {
                let path_without_slashes = manifest_path_str.replace('\\', "/");
                let remote_data =
                    request_remote_asset(&path_without_slashes, http_client, game_server_url)
                        .await
                        .map(|manifest| manifest.to_vec());
                if let Ok(remote_manifest) = remote_data {
                    decompress_asset_response(remote_manifest)
                        .map_err(|err| io::Error::new(ErrorKind::InvalidData, err.to_string()))?
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            };

            file_data.append(&mut remote_manifest);
            let crc = crc32fast::hash(&file_data);

            let manifest_path = path_without_prefix.with_file_name(MANIFEST_FILE_NAME);
            asset_map.insert(
                manifest_path,
                AssetLocator {
                    crc,
                    kind: AssetLocatorKind::Memory(MemoryAssetLocator { data: file_data }),
                },
            );

            let manifest_crc_path = path_without_prefix.with_file_name(MANIFEST_CRC_FILE_NAME);
            let crc_file_data = crc.to_string().as_bytes().to_vec();
            asset_map.insert(
                manifest_crc_path,
                AssetLocator {
                    crc: crc32fast::hash(&crc_file_data),
                    kind: AssetLocatorKind::Memory(MemoryAssetLocator {
                        data: crc_file_data,
                    }),
                },
            );
        } else if !file_name_ends_with(&path_without_prefix, MANIFEST_CRC_FILE_NAME) {
            let crc = crc32fast::hash(&file_data);

            // Always overwrite in-pack assets with assets outside a pack
            asset_map.insert(
                path_without_prefix,
                AssetLocator {
                    crc,
                    kind: AssetLocatorKind::File(FileAssetLocator {
                        path,
                        data_offset: 0,
                        size: file_data.len() as u32,
                    }),
                },
            );
        }
    }

    for task in tasks {
        let (path, assets) = task.await??;
        for asset in assets {
            asset_map.entry(asset.name).or_insert(AssetLocator {
                crc: asset.crc,
                kind: AssetLocatorKind::File(FileAssetLocator {
                    path: path.clone(),
                    data_offset: asset.data_offset,
                    size: asset.size,
                }),
            });
        }
    }

    Ok(asset_map)
}

fn decompose_extension(asset_name: &std::path::Path) -> (PathBuf, bool, Option<u32>) {
    let possible_extension_str = asset_name
        .extension()
        .map(|extension| extension.to_os_string().into_string().ok())
        .unwrap_or(None);
    let (non_crc_asset_name, crc) = if let Some(extension_str) = possible_extension_str {
        let extension_split = extension_str.rsplit_once(CRC_EXTENSION_SEPARATOR);

        if let Some((real_extension, crc_str)) = extension_split {
            (
                asset_name.with_extension(real_extension),
                crc_str.parse::<u32>().ok(),
            )
        } else {
            (asset_name.to_path_buf(), None)
        }
    } else {
        (asset_name.to_path_buf(), None)
    };

    let compressed = non_crc_asset_name
        .extension()
        .map(|extension| extension == COMPRESSED_EXTENSION)
        .unwrap_or(false);
    let uncompressed_asset_name = if compressed {
        non_crc_asset_name.with_extension("")
    } else {
        non_crc_asset_name.to_path_buf()
    };

    (uncompressed_asset_name, compressed, crc)
}

async fn build_local_asset_response(
    asset_locator: &AssetLocator,
    compress: bool,
) -> io::Result<Vec<u8>> {
    let mut buffer = Vec::new();

    let mut file_buffer = match &asset_locator.kind {
        AssetLocatorKind::Memory(locator) => locator.data.clone(),
        AssetLocatorKind::File(locator) => {
            // Read file from local client folder
            let mut file = OpenOptions::new().read(true).open(&locator.path).await?;
            file.seek(SeekFrom::Start(locator.data_offset)).await?;

            let mut file_buffer = vec![0; locator.size as usize];
            file.read_exact(&mut file_buffer).await?;
            file_buffer
        }
    };

    if compress {
        buffer.write_u32(COMPRESSED_MAGIC).await?;
        buffer.write_u32(file_buffer.len() as u32).await?;
        buffer.append(&mut compress_to_vec_zlib(
            &file_buffer,
            ZLIB_COMPRESSION_LEVEL,
        ));
    } else {
        buffer.append(&mut file_buffer);
    }

    Ok(buffer)
}

async fn request_remote_asset(
    path_and_query: &str,
    http_client: &Arc<Client>,
    game_server_url: &Arc<Url>,
) -> Result<Bytes, StatusCode> {
    let url = game_server_url
        .join("assets/")
        .and_then(|path| path.join(path_and_query))
        .map_err(|_| StatusCode::BAD_REQUEST)?;

    let response = http_client
        .get(url)
        .send()
        .await
        .map_err(|err| err.status().unwrap_or(StatusCode::INTERNAL_SERVER_ERROR))?;

    match response.status() {
        StatusCode::OK => Ok(response
            .bytes()
            .await
            .map_err(|err| err.status().unwrap_or(StatusCode::INTERNAL_SERVER_ERROR))?),
        status_code => Err(status_code),
    }
}

async fn retrieve_asset(
    asset_name: PathBuf,
    http_client: Arc<Client>,
    asset_map: Arc<AssetMap>,
    game_server_url: Arc<Url>,
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

    let (uncompressed_asset_name, compress, queried_crc) = decompose_extension(&asset_name);

    let possible_file_data = if let Some(asset_locator) = asset_map.get(&uncompressed_asset_name) {
        let crc = queried_crc.unwrap_or(asset_locator.crc);
        if crc == asset_locator.crc {
            build_local_asset_response(asset_locator, compress)
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
            .map(|path_and_query| {
                path_and_query
                    .path()
                    .strip_prefix("/assets/")
                    .expect("Assets request is missing /assets prefix")
            })
            .unwrap_or(request_path);
        request_remote_asset(path_and_query, &http_client, &game_server_url).await
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
    request: Request,
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

pub async fn prepare_proxy(
    port: u16,
    client_folder: &std::path::Path,
    game_server_uri: Url,
) -> io::Result<impl Future<Output = ()>> {
    let client = Client::new();
    let client_arc = Arc::new(client);
    let game_server_url_arc = Arc::new(game_server_uri.clone());
    let asset_map = build_asset_map(client_folder, &client_arc, &game_server_url_arc).await?;
    let app = Router::new()
        .route("/assets/*asset", get(asset_handler))
        .with_state((client_arc, Arc::new(asset_map), game_server_url_arc));

    let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).await?;
    println!(
        "Proxy listening on {}",
        listener.local_addr().expect("Listener has no address")
    );
    Ok(start_proxy(listener, app))
}
