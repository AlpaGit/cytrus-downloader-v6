use std::collections::HashMap;
use serde::{Deserialize};

#[derive(Deserialize)]
pub struct CytrusRoot {
    pub name: String,
    pub version: u16,
    pub games: HashMap<String, GameRoot>,
}

#[derive(Deserialize)]
pub struct GameRoot {
    pub name: String,
    pub order: u16,
    #[serde(rename = "gameId")]
    pub game_id: u16,
    pub platforms: HashMap<String, HashMap<String, String>>,
}


pub struct Manifest {
    pub fragments: Vec<Fragment>,
}

pub struct Fragment {
    pub name: String,
    pub files: Vec<FileM>,
    pub bundles: Vec<Bundle>,
}

pub struct FileM {
    pub name: String,
    pub size: u64,
    pub hash: String,
    pub chunks: Vec<Chunk>,
    pub executable: bool,
    pub symlink: String,
}

pub struct Bundle {
    pub hash: String,
    pub chunks: Vec<Chunk>,
}

pub struct Chunk {
    pub size: u64,
    pub hash: String,
    pub offset: u64,
}