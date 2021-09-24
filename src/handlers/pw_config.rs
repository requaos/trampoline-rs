use crate::rpc::*;
use crate::rpc::{get_pw_tx_info, get_sudt_tx_info};
use crate::DEV_RPC_URL;
use anyhow::Result;
use ckb_jsonrpc_types::{
    CellDep as RpcCellDep, DepType, JsonBytes, OutPoint as RpcOutpoint, Script as RpcScript,
    ScriptHashType, TransactionWithStatus, Uint32,
};
use ckb_sdk::{Address, AddressPayload, AddressType};
use ckb_types::{bytes::Bytes, core::{self, ScriptHashType as CoreScriptHashType}, prelude::*, H256};
use ckb_hash::new_blake2b;
use serde::{Deserialize, Serialize};
use serde_json;
use std::borrow::Borrow;
use std::convert::TryFrom;
use std::fs;
use std::path::Path;
use std::str::FromStr;
use toml;
use ckb_types::packed::{Byte32, Byte};


#[derive(Clone, Debug, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Script {
    code_hash: H256,
    hash_type: ScriptHashType,
    args: JsonBytes,
}

#[derive(Clone, Debug, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OutPoint {
    tx_hash: H256,
    index: Uint32,
}

#[derive(Clone, Debug, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CellDep {
    out_point: OutPoint,
    dep_type: DepType,
}
#[derive(Clone, Debug, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PwScriptRef {
    cell_dep: CellDep,
    script: Script,
}
#[derive(Clone, Debug, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DappConfig {
    dev: PwConfig,
}

#[derive(Clone, Debug, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PwConfig {
    dao_type: PwScriptRef,
    default_lock: PwScriptRef,
    pw_lock: PwScriptRef,
    sudt_type: PwScriptRef,
    multi_sig_lock: PwScriptRef,
    acp_lock_list: Vec<Script>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ChainConfig {
    pub ckb_dev: DevConfig,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DevConfig {
    pub spec_hash: String,
    pub genesis: String,
    pub cellbase: String,
    pub dep_groups: Vec<DepGroupConfig>,
    pub system_cells: Vec<SysCellConfig>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SysCellConfig {
    pub path: String,
    pub tx_hash: String,
    pub index: u32,
    pub data_hash: String,
    pub type_hash: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DepGroupConfig {
    pub included_cells: Vec<String>,
    pub tx_hash: String,
    pub index: u32,
}

pub fn read_hash_toml() -> Result<ChainConfig> {
    let toml_ = fs::read_to_string("./ckb-hashes.toml")?;
    let decoded: ChainConfig = toml::from_str(toml_.as_str())?;
    let as_json = serde_json::to_string(&decoded)?;
    let as_json = as_json.as_str();
    fs::write("./ckb-hashes.json", as_json)?;
    Ok(decoded)
}

#[derive(Debug, Deserialize, Serialize)]
struct RawOutpoint {
    pub tx_hash: String,
    pub code_hash: String,
    pub index: u32,
}

enum SysCellSelected {
    DaoType,
    DefaultLock,
    PwLock,
    Sudt,
}

// To do: Remove using ckb_sdk types and take advantage of serialization implementations.
// ckb_sdk types do not serialize exactly as required
pub fn gen_config() -> Result<DappConfig> {
    let hashes_json = fs::read_to_string("./ckb-hashes.json")?;
    let chain_config: ChainConfig = serde_json::from_str(hashes_json.as_str())?;
    let sys_cells = &chain_config.ckb_dev.system_cells;
    let dao_type = gen_syscell_config(sys_cells, SysCellSelected::DaoType)?;
    let default_lock = gen_syscell_config(sys_cells, SysCellSelected::DefaultLock)?;
    let sudt_type = gen_syscell_config(sys_cells, SysCellSelected::Sudt)?;
    let pw_lock = gen_syscell_config(sys_cells, SysCellSelected::PwLock)?;
    let multi_sig_lock = gen_multisig_config()?;
    let acp_lock_list = gen_acp_lock_list_config()?;

    let pw_config = PwConfig {
        dao_type,
        default_lock,
        pw_lock,
        sudt_type,
        multi_sig_lock,
        acp_lock_list,
    };

    let dapp_config = DappConfig { dev: pw_config };

    fs::write("./PwConfig.json", serde_json::to_string(&dapp_config)?)?;
    Ok(dapp_config)
}

fn gen_acp_lock_list_config() -> Result<Vec<Script>> {
    let mut vec = Vec::new();
    let script = Script {
        code_hash: Default::default(),
        args: Default::default(),
        hash_type: ScriptHashType::Type,
    };
    vec.push(script);

    Ok(vec)
}
fn gen_multisig_config() -> Result<PwScriptRef> {
    let out_point = OutPoint {
        tx_hash: H256::from_str(
            "d6d78382f948a6fab16ba084a4c3ed16eb3fe203669a6bc8a8f831e09177117f",
        )?,
        index: Uint32::from(1),
    };

    let cell_dep = CellDep {
        out_point,
        dep_type: DepType::DepGroup,
    };

    let script = Script {
        code_hash: H256::from_str(
            "5c5069eb0857efc65e1bca0c07df34c31663b3622fd3876c876320fc9634e2a8",
        )?,
        hash_type: ScriptHashType::Type,
        args: Default::default(),
    };

    Ok(PwScriptRef { cell_dep, script })
}
fn gen_syscell_config(
    sys_cells: &Vec<SysCellConfig>,
    type_: SysCellSelected,
) -> Result<PwScriptRef> {
    return match type_ {
        SysCellSelected::DaoType => gen_dao_config(sys_cells),
        SysCellSelected::DefaultLock => gen_default_lock_config(sys_cells),
        SysCellSelected::Sudt => gen_sudt_config(),
        SysCellSelected::PwLock => gen_pwlock_config(),
    };
}

fn gen_dao_config(sys_cells: &Vec<SysCellConfig>) -> Result<PwScriptRef> {
    let mut raw_dao_out = RawOutpoint {
        tx_hash: sys_cells[1].tx_hash.clone(),
        code_hash: sys_cells[1].type_hash.as_ref().unwrap().clone(),
        index: sys_cells[1].index.clone(),
    };

    let dao_script = build_script(&raw_dao_out.code_hash, "type", "0x")?;
    let dao_cell_dep = build_cell_dep(&raw_dao_out.tx_hash, raw_dao_out.index, "code")?;

    let dao_pw_obj = PwScriptRef {
        script: dao_script,
        cell_dep: dao_cell_dep,
    };
    // fs::write("./pw-config-dao.json", serde_json::to_string(&dao_pw_obj)?)?;

    Ok(dao_pw_obj)
}

fn gen_default_lock_config(sys_cells: &Vec<SysCellConfig>) -> Result<PwScriptRef> {
    let mut raw_lock_out = RawOutpoint {
        tx_hash: sys_cells[0].tx_hash.clone(),
        code_hash: sys_cells[0].type_hash.as_ref().unwrap().clone(),
        index: sys_cells[0].index.clone(),
    };

    let lock_script = build_script(&raw_lock_out.code_hash, "type", "0x")?;
    let lock_dep = build_cell_dep(&raw_lock_out.tx_hash, raw_lock_out.index, "code")?;

    let lock_pw_obj = PwScriptRef {
        script: lock_script,
        cell_dep: lock_dep,
    };

    // fs::write(
    //     "./pw-config-default-lock.json",
    //     serde_json::to_string(&lock_pw_obj)?,
    // )?;

    Ok(lock_pw_obj)
}

// To do: code_hash should be hash of the script attached to sudt type output, not
// the code hash contained within the sudt output type script.
fn gen_sudt_config() -> Result<PwScriptRef> {
    let sudt_info = get_sudt_tx_info(DEV_RPC_URL)?;
    let tx_hash = sudt_info.transaction.hash;
    let index: u32 = 0;
    let code_hash = sudt_info.transaction.inner.outputs[0]
        .type_
        .as_ref()
        .unwrap()
        .code_hash
        .clone();
    let args = sudt_info.transaction.inner.outputs[0]
        .type_
        .as_ref()
        .unwrap()
        .args
        .clone();

    // let args = hex::decode(args.trim_start_matches("0x"))?;
    // let args = Bytes::copy_from_slice(args.as_slice());
    // println!("Args as bytes: {:?}", args);
    // let args = JsonBytes::from_bytes(args);

    let script = Script {
        code_hash,
        hash_type: ScriptHashType::Type,
        args,
    };
    let script = gen_script_dep(script)?;

    let index = Uint32::from(index);
    let out_point = OutPoint { tx_hash, index };

    let cell_dep = CellDep {
        out_point,
        dep_type: DepType::Code,
    };

    let sudt_pw_obj = PwScriptRef { script, cell_dep };
    //
    // fs::write(
    //     "./pw-config-sudt.json",
    //     serde_json::to_string(&sudt_pw_obj)?,
    // )?;
    Ok(sudt_pw_obj)
}

pub fn gen_script_dep(type_script_on_dep: Script) -> Result<Script> {
   // args as default
    // code_hash as the hash of the input script
    // hashtype = type
    // To hash the script, first must serialize the script appropriately.
    // First, Byte32 as code hash
    // Second, Byte as hash_type,
    // Third, Bytes as args
    let mut script = ckb_jsonrpc_types::Script::default();
    script.hash_type = type_script_on_dep.hash_type;
    script.code_hash = type_script_on_dep.code_hash;
    script.args = type_script_on_dep.args;

    let packed_script = ckb_types::packed::Script::from(script);
    let mut script_hash = packed_script.calc_script_hash();

    Ok(Script {
        code_hash: script_hash.unpack(),
        hash_type: ScriptHashType::Type,
        args: JsonBytes::default(),
    })
}

fn gen_pwlock_config() -> Result<PwScriptRef> {
    let pwlock_info = get_pw_tx_info(DEV_RPC_URL)?;
    let tx_hash = pwlock_info.clone().transaction.hash;
    let index: u32 = 0;
   // println!("PW LOCK TX: {:?}", pwlock_info.transaction.inner);
    let code_hash = pwlock_info.clone().transaction.inner.outputs[0]
        .type_
        .as_ref()
        .unwrap()
        .code_hash
        .clone();

    println!("CODE HASH: {:?}", code_hash.to_string());

    let args = pwlock_info.clone().transaction.inner.outputs[0]
        .type_
        .as_ref()
        .unwrap()
        .args
        .clone();
    println!("ARGS: {:?}", hex::encode(args.as_bytes()));
    // let args = hex::decode(args.trim_start_matches("0x"))?;
    // let args = Bytes::copy_from_slice(args.as_slice());
    // println!("Args as bytes: {:?}", args);
    // let args = JsonBytes::from_bytes(args);

    // Gen address
    let script = Script {
        code_hash,
        hash_type: ScriptHashType::Type,
        args,
    };

    let script = gen_script_dep(script)?;
    println!("SCRIPT DEP: {:?}", script);
    let index = Uint32::from(index);
    println!("INDEX: {:?}", index);
    let out_point = OutPoint { tx_hash, index };

    let cell_dep = CellDep {
        out_point,
        dep_type: DepType::Code,
    };

    let pw_lock_obj = PwScriptRef { script, cell_dep };

    // fs::write(
    //     "./pw-config-pw-lock.json",
    //     serde_json::to_string(&pw_lock_obj)?,
    // )?;
    Ok(pw_lock_obj)
}
fn build_cell_dep(tx_hash: &str, index: u32, dep_type: &str) -> Result<CellDep> {
    let tx_hash = tx_hash.trim_start_matches("0x");
    let dep_type = match dep_type {
        "code" => DepType::Code,
        "group" => DepType::DepGroup,
        _ => DepType::Code,
    };

    let index = Uint32::from(index);

    let out_point = OutPoint {
        tx_hash: H256::from_str(tx_hash)?,
        index,
    };

    Ok(CellDep {
        out_point,
        dep_type,
    })
}
fn build_script(code_hash: &str, hash_type: &str, args: &str) -> Result<Script> {
    let code_hash = code_hash.trim_start_matches("0x");
    let code_hash = H256::from_str(code_hash)?;

    let script_hash_type = match hash_type {
        "type" => ScriptHashType::Type,
        "data" => ScriptHashType::Data,
        _ => {
            panic!("Invalid hash type");
        }
    };

    let args = hex::decode(args.trim_start_matches("0x"))?;
    let args = Bytes::copy_from_slice(args.as_slice());
    println!("Args as bytes: {:?}", args);
    let args = JsonBytes::from_bytes(args);
    println!("Args as json bytes: {:?}", args);

    Ok(Script {
        code_hash,
        hash_type: script_hash_type,
        args,
    })
}
