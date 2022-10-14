use std::{
  fs::{self, File},
  io,
  path::PathBuf,
};

use anyhow::{anyhow, Result};
use lapce_plugin::{
  psp_types::{
    lsp_types::{request::Initialize, DocumentFilter, InitializeParams, Url},
    Request,
  },
  register_plugin, Http, LapcePlugin, VoltEnvironment, PLUGIN_RPC,
};
use serde_json::Value;
use zip::ZipArchive;

#[derive(Default)]
struct State {}

register_plugin!(State);

macro_rules! string {
  ( $x:expr ) => {
    String::from($x)
  };
}

macro_rules! ok {
  ( $x:expr ) => {
    match ($x) {
      | Ok(v) => v,
      | Err(e) => return Err(anyhow!(e)),
    }
  };
}

fn initialize(params: InitializeParams) -> Result<()> {
  let mut terraform_version = "0.29.1";
  let document_selector = vec![
    DocumentFilter {
      language: Some(string!("terraform")),
      pattern: Some(string!("**/*.tf")),
      scheme: None,
    },
    DocumentFilter {
      language: Some(string!("terraform")),
      pattern: Some(string!("**/*.tfvars")),
      scheme: None,
    },
  ];
  let mut server_args = vec![string!("serve")];

  if let Some(options) = params.initialization_options.as_ref() {
    if let Some(volt) = options.get("volt") {
      if let Some(args) = volt.get("serverArgs") {
        if let Some(args) = args.as_array() {
          if !args.is_empty() {
            server_args = vec![];
          }
          for arg in args {
            if let Some(arg) = arg.as_str() {
              server_args.push(string!(arg));
            }
          }
        }
      }

      if let Some(server_path) = volt.get("serverPath") {
        if let Some(server_path) = server_path.as_str() {
          if !server_path.is_empty() {
            let server_uri = ok!(Url::parse(&format!("urn:{}", server_path)));
            PLUGIN_RPC.start_lsp(
              server_uri,
              server_args,
              document_selector,
              params.initialization_options,
            );
            return Ok(());
          }
        }
      }

      if let Some(ver) = options.get("terraformVersion") {
        if let Some(ver) = ver.as_str() {
          if !ver.is_empty() {
            terraform_version = ver
          }
        }
      }
    }
  }

  let arch = match VoltEnvironment::architecture().as_deref() {
    | Ok("x86") => "386",
    | Ok("x86_64") => "amd64",
    | Ok("aarch64") => "arm64",
    | Ok(v) => return Err(anyhow!("Unsupported ARCH: {}", v)),
    | Err(e) => return Err(anyhow!("Error ARCH: {}", e)),
  };

  let zip_file = match VoltEnvironment::operating_system().as_deref() {
    | Ok("macos") => format!("terraform-ls_{terraform_version}_darwin_{arch}.zip"),
    | Ok("linux") => format!("terraform-ls_{terraform_version}_linux_{arch}.zip"),
    | Ok("windows") => format!("terraform-ls_{terraform_version}_windows_{arch}.zip"),
    | Ok("openbsd") => format!("terraform-ls_{terraform_version}_openbsd_{arch}.zip"),
    | Ok("freebsd") => format!("terraform-ls_{terraform_version}_freebsd_{arch}.zip"),
    | Ok(v) => return Err(anyhow!("Unsupported OS: {}", v)),
    | Err(e) => return Err(anyhow!("Error OS: {}", e)),
  };

  PLUGIN_RPC.stderr(&format!("ZIP_FILE: {}", zip_file));

  let zip_file = PathBuf::from(zip_file);

  let download_url = format!(
    "https://github.com/hashicorp/terraform-ls/releases/download/v{terraform_version}/{}",
    zip_file.display()
  );

  let server_path = match VoltEnvironment::operating_system().as_deref() {
    | Ok("windows") => PathBuf::from("terraform-ls.exe"),
    | _ => PathBuf::from("terraform-ls"),
  };

  if !PathBuf::from(&server_path).exists() {
    if zip_file.exists() {
      fs::remove_file(&zip_file)?;
    }
    let mut resp = Http::get(&download_url)?;
    PLUGIN_RPC.stderr(&format!("STATUS_CODE: {:?}", resp.status_code));
    if resp.status_code.is_success() {
      let body = resp.body_read_all()?;
      fs::write(&zip_file, body)?;

      let mut zip = ZipArchive::new(File::open(&zip_file)?)?;

      for i in 0..zip.len() {
        let mut file = zip.by_index(i)?;
        let outpath = match file.enclosed_name() {
          | Some(path) => path.to_owned(),
          | None => continue,
        };

        if (*file.name()).ends_with('/') {
          fs::create_dir_all(&outpath)?;
        } else {
          if let Some(p) = outpath.parent() {
            if !p.exists() {
              fs::create_dir_all(&p)?;
            }
          }
          let mut outfile = File::create(&outpath)?;
          io::copy(&mut file, &mut outfile)?;
        }
      }
    }

    fs::remove_file(&zip_file)?;
  }

  let volt_uri = VoltEnvironment::uri()?;
  let server_uri = Url::parse(&volt_uri)?.join(server_path.to_str().unwrap_or("./terraform-ls"))?;

  PLUGIN_RPC.start_lsp(
    server_uri,
    server_args,
    document_selector,
    params.initialization_options,
  );

  Ok(())
}

impl LapcePlugin for State {
  fn handle_request(&mut self, _id: u64, method: String, params: Value) {
    #[allow(clippy::single_match)]
    match method.as_str() {
      | Initialize::METHOD => {
        let params: InitializeParams = serde_json::from_value(params).unwrap();
        if let Err(e) = initialize(params) {
          PLUGIN_RPC.stderr(&format!("plugin returned with error: {e}"))
        }
      }
      | _ => {}
    }
  }
}
