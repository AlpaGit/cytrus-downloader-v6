use std::{env, fs};
use std::fs::{File, OpenOptions, remove_file};
use std::io::{Write, Read, SeekFrom, Seek};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use crate::models::{Bundle, Chunk, CytrusRoot, FileM, Fragment, Manifest};
mod models;
use flatbuffers;
use flatbuffers::Vector;
use crate::manifest_generated::{ManifestFb};
use rayon::iter::IntoParallelRefIterator;
use futures_util::{FutureExt, StreamExt};
use futures_util::future::join_all;
use futures_util::stream::FuturesUnordered;
use rayon::prelude::*;
use tokio::task::spawn_blocking;

#[allow(dead_code, unused_imports)]
#[path = "./flatbuffers/manifest_generated.rs"]
mod manifest_generated;

const CYTRUS_VERSION: u16 = 6;
const CYTRUS_URL: &str = "https://cytrus.cdn.ankama.com/cytrus.json";

const DEFAULT_GAME: &str = "dofus";
const DEFAULT_PLATFORM: &str = "windows";
const DEFAULT_RELEASE: &str = "main";

const DEFAULT_DIR_OUT: &str = "./out";

#[tokio::main]
async fn main() -> ExitCode {
    // read afile calld "manifest.bin"

    match entry().await {
        Ok(_) => ExitCode::SUCCESS,
        Err(_) => ExitCode::FAILURE,
    }
}

async fn entry() -> Result<(), ()> {
    let args: Vec<String> = env::args().collect();
    let program = &args[0];

    if args.len() < 1 {
        usage(&program);
        return Ok(());
    }

    let sub_command = &args[1];

    if(sub_command == "download"){
        download_from_args(&args).await.expect("Could not download the game");
    }
    else{
        eprintln!("ERROR: unknown subcommand: {}", sub_command);
        usage(&program);
    }

    Ok(())
}

fn usage(program: &str) {
    eprintln!("Usage: {program} [SUBCOMMAND] [OPTIONS]");
    eprintln!("Subcommands:");
    eprintln!("    download [game] [version] [platform] [release]      download the <game> with");
    eprintln!("                                                        the <version> or latest if not specified");
    eprintln!("                                                        on the <platform> or windows if not specified [windows|linux|darwin]");
    eprintln!("                                                        on the <release> or main if not specified [main|beta]");
}

async fn download_from_args(args: &Vec<String>) -> Result<(), ()> {
    let game = &args[2];
    let mut version =  (&args[3]).to_string();
    let platform = &args[4];
    let release = &args[5];

    if version == "0" {
        version = get_latest_version(&game, &platform, &release).await?;
    }

    download(&game, &version, &platform).await.map_err(|err| {
        eprintln!("ERROR: could not download the game: {:?}", err);
        ()
    })?;
    Ok(())
}

async fn download(game: &str, version: &str, platform:&str) -> Result<(), ()> {
    println!("Downloading {} version {}", game, version);
    
    let manifest = get_manifest(game, version, platform, "main").await?;
 k     println!("Manifest downloaded");



    let out_path = &Path::new(DEFAULT_DIR_OUT)
                                    .join(game)
                                    .join(platform);
    
    create_dir_all(&out_path)?;

    for fragment in manifest.fragments {
        let fragment_path = Path::join(out_path, fragment.name);

        create_dir_all(&fragment_path)?;
        
        // TODO: downloading bundles can be a bit difficult to implement maybe do it later
        download_bundles(game.to_string(), &fragment_path, fragment.files, fragment.bundles).await?;
        
        //download_files(game.to_string(), &fragment_path, &fragment.files).await?;
    }
    Ok(())
}

async fn download_files(game:String, path:&Path, files: &Vec<FileM>) -> Result<(), ()> {
    for file in files {
        let file_path = Path::join(path, &file.name);
        
        if file_path.exists() {
            let current_hash = sha1(&file_path)?;
            if current_hash == file.hash {
                println!("File {} is already up to date", file.name);
                continue;
            }
            
            println!("File {} is not up to date, downloading it ({}, {})", file.name, current_hash, file.hash);
        }
        
        let url = format!("https://cytrus.cdn.ankama.com/{game}/hashes/{hash_pref}/{hash}",
                           game=game, hash_pref=&file.hash[0..2], hash=file.hash);
        
        println!("Downloading file {} ({url})", file.name);
        
        let res = reqwest::get(url)
            .await.map_err(|err| {
                  eprintln!("ERROR: could not download the file: {err}");
                  ()
            })?;
        
        create_dir_all(&file_path.parent().unwrap())?;
        
        let mut file_disk = File::create(&file_path).map_err(|err| {
            eprintln!("ERROR: could not create the file: {path} ({err})", path = file_path.display());
            ()
        })?;

        file_disk.write_all(&res.bytes().await.map_err(|err| {
            eprintln!("ERROR: could not write the file: {path} ({err})", path = file_path.display());
            ()
        })?).map_err(|err| {
            eprintln!("ERROR: could not write the file: {path} ({err})", path = file_path.display());
            ()
        })?;
        
        println!("File {} downloaded", &file.name);
    }
    
    Ok(())
}



async fn download_bundles(game:String, path:&Path, files: Vec<FileM>, bundles: Vec<Bundle>) -> Result<(), ()> {

    let mut futures = FuturesUnordered::new();
    for bundle in bundles {
        let arc = Arc::new(bundle);
        futures.push(download_bundle(&game, &path, &files, arc.clone()));
    }
    
    while let Some(result) = futures.next().await {
        result?;
    }
    
    Ok(())
}

async fn download_bundle<'a>(game: &str, path: &Path, files: &Vec<FileM>, bundle: Arc<Bundle>) -> Result<(), ()> {
    let bundle_path = Path::join(path, &bundle.hash);

    if bundle_path.exists() {
        let current_hash = sha1(&bundle_path)?;
        if current_hash == bundle.hash {
            println!("Bundle {} is already up to date", bundle.hash);
            return Ok(());
        } else {
            println!("Bundle {} is not up to date, downloading it ({}, {})", bundle.hash, current_hash, bundle.hash);
        }
    }

    let bundle_path = &path.join(&bundle.hash);

    let url = &format!("https://cytrus.cdn.ankama.com/{game}/bundles/{}/{}", &bundle.hash[..2], &bundle.hash);

    println!("Downloading bundle {} ({url})", bundle.hash);

    let res = reqwest::get(url)
        .await.map_err(|err| {
        eprintln!("ERROR: could not download the bundle: {err}");
        ()
    })?;

    let mut file = File::create(bundle_path).map_err(|err| {
        eprintln!("ERROR: could not create the file: {path} ({err})", path = bundle_path.display());
        ()
    })?;

    let mut stream = res.bytes_stream();

    while let Some(item) = stream.next().await {
        let bytes = item.map_err(|err| {
            eprintln!("ERROR: could not read the bundle: {err}");
            ()
        })?;

        file.write_all(&bytes).map_err(|err| {
            eprintln!("ERROR: could not write the bundle: {err}");
            ()
        })?;
    }

    println!("Bundle {} downloaded", bundle.hash);

    let res = &bundle.chunks.iter().map(|chunk| {
        extract_bundle_chunks(&path, &files, &bundle_path, chunk)
    }).collect::<Vec<Result<(), ()>>>();

    if res.iter().any(|res| res.is_err()) {
        eprintln!("ERROR: could not extract the bundle: {path}", path = bundle_path.display());
        return Err(());
    }

    //clean the disk
    remove_file(bundle_path).map_err(|err| {
        eprintln!("ERROR: could not remove the file: {path} ({err})", path = bundle_path.display());
        ()
    })?;
    
    Ok(())
}

fn extract_bundle_chunks(path: &Path, files: &Vec<FileM>, bundle_path: &PathBuf, chunk: &Chunk) -> Result<(), ()> {
    let files = get_files_chunks_concerned(&chunk.hash, files);

    println!("DEBUG: chunk {hash} is concerned by {nb} files", hash = chunk.hash, nb = files.len());

    // we get the buffer chunk from the bundle
    let mut file = File::open(&bundle_path).map_err(|err| {
        eprintln!("ERROR: could not open the bundle: {path} ({err})", path = &bundle_path.display());
        ()
    })?;

    file.seek(SeekFrom::Start(chunk.offset as u64)).map_err(|err| {
        eprintln!("ERROR: could not seek the bundle: {path} ({err})", path = &bundle_path.display());
        ()
    })?;

    let mut buffer = vec![0; chunk.size as usize];
    file.read_exact(&mut buffer).map_err(|err| {
        eprintln!("ERROR: could not read the bundle: {path} ({err})", path = &bundle_path.display());
        ()
    })?;
    
    // we have to write every chunks of every files
    for (file, chunk_file) in files {
        let file_path = Path::join(path, &file.name);
        create_dir_all(&file_path.parent().unwrap()).unwrap();

        println!("DEBUG: writing chunk {hash} of file {file} at {offset}..{size}",
                 hash = chunk.hash, file = file_path.display(), offset = chunk_file.offset, size = chunk_file.size);

        let mut file_disk = OpenOptions::new().create(true).write(true).open(&file_path).map_err(|err| {
            eprintln!("ERROR: could not create the file: {path} ({err})", path = &file_path.display());
            ()
        })?;
        
        file_disk.seek(SeekFrom::Start(chunk_file.offset as u64)).map_err(|err| {
            eprintln!("ERROR: could not seek the file: {path} ({err})", path = &file_path.display());
            ()
        })?;

        file_disk.write_all(&buffer).map_err(|err| {
            eprintln!("ERROR: could not write the file: {path} ({err})", path = &file_path.display());
            ()
        })?;
        
        file_disk.flush().map_err(|err| {
            eprintln!("ERROR: could not flush the file: {path} ({err})", path = &file_path.display());
            ()
        })?;
    }

    Ok(())
}

async fn get_latest_version(game:&str, platform:&str, release:&str) -> Result<String, ()> {
    let req = reqwest::get(CYTRUS_URL)
        .await
        .map_err(|err| {
            eprintln!("ERROR: could not fetch the url: {}", err);
            ()
        })?;
    
    let body:CytrusRoot = req.json().await.map_err(|err| {
        eprintln!("ERROR: could not parse the json: {}", err);
        eprintln!("ERROR: is the url {} correct?", CYTRUS_URL);
        ()
    })?;
    
    if body.version != CYTRUS_VERSION {
        eprintln!("ERROR: the cytrus version is not supported");
        eprintln!("ERROR: expected {}, got {}", CYTRUS_VERSION, body.version);
        return Err(());
    }
    
    let game = body.games.get(game).ok_or_else(|| {
        eprintln!("ERROR: could not find the game {}", game);
        ()
    })?;
    
    let platform = game.platforms.get(platform).ok_or_else(|| {
        eprintln!("ERROR: could not find the platform {}", platform);
        ()
    })?;
    
    let release = platform.get(release).ok_or_else(|| {
        eprintln!("ERROR: could not find the release {}", release);
        ()
    })?;
    
    Ok(release.to_string())
}

async fn get_manifest<'a>(game: &str, version: &str, platform: &str, release: &str) -> Result<Manifest, ()> {
    let req = reqwest::get(format!("https://cytrus.cdn.ankama.com/{game}/releases/{release}/{platform}/{version}.manifest",
                                   game=game, version=version, platform=platform, release=release))
        .await
        .map_err(|err| {
            eprintln!("ERROR: could not fetch the url: {}", err);
            ()
        })?;

    let bytes = req.bytes().await.map_err(|err| {
        eprintln!("ERROR: could not parse the json: {}", err);
        ()
    })?;
    
    let bytes = bytes.to_vec();

    let file = File::open("manifest.manifest").map_err(|err| {
        eprintln!("ERROR: could not open the file: {}", err);
        ()
    }).unwrap();

    // get the bytes
    let bytes = file.bytes().map(|byte| byte.unwrap()).collect::<Vec<u8>>();

    let manifest_fb = flatbuffers::root::<ManifestFb>(&bytes).map_err(|err| {
        eprintln!("ERROR: could not parse the manifest: {}", err);
        ()
    })?;
    
    let mut manifest = Manifest {
        fragments: vec![],
    };
    
    match manifest_fb.fragments() {
        Some(fragments) => {
            for fragment_fb in fragments {
                let mut fragment = Fragment {
                    name: fragment_fb.name().unwrap().to_string(),
                    files: vec![],
                    bundles: vec![],
                };
                
                match fragment_fb.files() {
                    Some(files) => {
                        for file_fb in files {
                            let mut file = FileM {
                                name: file_fb.name().unwrap().to_string(),
                                size: file_fb.size_() as u64,
                                // buffer to string
                                hash: vec_to_hex_string(file_fb.hash().unwrap()),
                                chunks: vec![],
                                executable: file_fb.executable(),
                                symlink: match file_fb.symlink() {
                                    Some(symlink) => symlink.to_string(),
                                    None => String::from(""),
                                }
                            };
                            
                            match file_fb.chunks() {
                                Some(chunks) => {
                                    for chunk_fb in chunks {
                                        let chunk = Chunk {
                                            size: chunk_fb.size_() as u64,
                                            // buffer to string
                                            hash: vec_to_hex_string(chunk_fb.hash().unwrap()),
                                            offset: chunk_fb.offset() as u64,
                                        };
                                        
                                        file.chunks.push(chunk);
                                    }
                                },
                                None => {
                                    //eprintln!("ERROR: could not find any chunks");
                                    //return Err(());
                                }
                            }

                            fragment.files.push(file);
                        }
                    },
                    None => {
                        eprintln!("ERROR: could not find any files");
                        return Err(());
                    }
                }
                
                match fragment_fb.bundles() {
                    Some(bundles) => {
                        for bundle_fb in bundles {
                            let mut bundle = Bundle {
                                hash: vec_to_hex_string(bundle_fb.hash().unwrap()),
                                chunks: vec![],
                            };
                            
                            match bundle_fb.chunks() {
                                Some(chunks) => {
                                    for chunk_fb in chunks {
                                        let chunk = Chunk {
                                            size: chunk_fb.size_() as u64,
                                            hash: vec_to_hex_string(chunk_fb.hash().unwrap()),
                                            offset: chunk_fb.offset() as u64,
                                        };
                                        
                                        bundle.chunks.push(chunk);
                                    }
                                },
                                None => {
                                    eprintln!("ERROR: could not find any chunks");
                                    return Err(());
                                }
                            }

                            fragment.bundles.push(bundle);
                        }
                    },
                    None => {
                        eprintln!("ERROR: could not find any bundles");
                        return Err(());
                    }
                }

                manifest.fragments.push(fragment);
            }
        },
        None => {
            eprintln!("ERROR: could not find any fragments");
            return Err(());
        }
    }
                
    Ok(manifest)
}

fn vec_to_hex_string(vec: Vector<i8>) -> String {
    let mut hex_string = String::new();
    for byte in vec {
        hex_string.push_str(&format!("{:02x}", byte as u8));
    }
    
    hex_string
}

fn create_dir_all(path: &Path) -> Result<(), ()> {
    if path.exists() {
        return Ok(());
    }
    
    println!("INFO: creating the directory {path}", path = path.display());
    
    fs::create_dir_all(path).map_err(|err| {
        eprintln!("ERROR: could not create the directory {path}: {err}", path = path.display(), err = err);
        ()
    })
}

// Maybe use later to update the game
fn get_bytes_ranges(bundle:&Bundle) -> String {
    let mut bytes = String::from("bytes=");
    let chunks = bundle.chunks
        .iter()
        .map(|chunk| format!("{}-{}", chunk.offset, chunk.offset + chunk.size - 1));
    
    for chunk in chunks {
        bytes.push_str(&format!("{},", chunk));
    }
    
    bytes.pop();
    bytes
}

fn sha1(file_path: &PathBuf) -> Result<String, ()> {
    let mut hasher = sha1_smol::Sha1::new();
    
    // read the file by chunks
    let mut file = File::open(file_path).map_err(|err| {
        eprintln!("ERROR: could not open the file {path}: {err}", path = file_path.display(), err = err);
        ()
    })?;
    
    let mut buffer = [0; 1024];
    loop {
        let count = file.read(&mut buffer).unwrap();
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    
    // read one time
    
    /*let mut buffer = Vec::new();
    let read = file.read_to_end(&mut buffer).map_err(|err| {
        eprintln!("ERROR: could not read the file {path}: {err}", path = file_path.display(), err = err);
        ()
    })?;
    
    hasher.update(&buffer[..read]);*/
    Ok(hasher.digest().to_string())
    //Ok(format!("{:x}", end))
}

fn get_files_chunks_concerned<'a>(hash:&str, files: &'a Vec<FileM>) -> Vec<(&'a FileM, Chunk)> {
    let mut files_chunks:Vec<(&FileM, Chunk)> = vec![];
    
    for file in files {
        if file.chunks.len() == 0 && file.hash == hash {
            files_chunks.push((file, Chunk {
                size: file.size,
                hash: file.hash.clone(),
                offset: 0,
            }));
            continue;
        }
        
        for chunk in &file.chunks {
            if chunk.hash == hash {
                let c = Chunk {
                    size: chunk.size,
                    hash: chunk.hash.clone(),
                    offset: chunk.offset,
                };
                files_chunks.push((file, c));
            }
        }
    }

    files_chunks
}