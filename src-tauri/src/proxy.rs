use std::collections::{HashMap, VecDeque};
use std::io::SeekFrom;
use std::path::PathBuf;

use tokio::{io, spawn};
use tokio::fs::{OpenOptions, read, read_dir};
use tokio::io::{AsyncReadExt, AsyncSeekExt};

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
