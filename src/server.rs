use hyper::header::{
    HeaderValue, ACCESS_CONTROL_ALLOW_HEADERS, ACCESS_CONTROL_ALLOW_METHODS,
    ACCESS_CONTROL_ALLOW_ORIGIN, CONTENT_TYPE,
};
use hyper::{http::response::Builder, http::Error, Body, Request, Response, StatusCode};
use json5;
use serde_json::Value;
use std::collections::HashMap;
use std::convert::Infallible;
use std::fs;
use std::path::Path;

use crate::config::{Config, HeaderConfig, HeaderId, PathConfig, UrlPath};

pub async fn handle(req: Request<Body>, config: Config) -> Result<Response<Body>, Error> {
    match handle_always(&config.always) {
        Some(x) => return Ok(x.unwrap()),
        _ => (),
    }

    let path = uri_path(req.uri().path());
    if let Some(paths) = &config.paths {
        let url_path = handle_static_path(path, paths, &config.headers);
        match url_path {
            Some(x) => return Ok(x.unwrap()),
            _ => (),
        }
    }
    handle_dyn_path(path, &config.dyn_data_dir.clone().unwrap().as_str())
        .expect("Unknown error occurred")
}

fn handle_always(always: &Option<String>) -> Option<Result<Response<Body>, Infallible>> {
    match always {
        Some(x) => {
            let response = json_response_base(&None, &None)
                .body(Body::from(x.to_owned()))
                .expect("Invalid always value");
            Some(Ok(response))
        }
        _ => None,
    }
}

fn uri_path(uri_path: &str) -> &str {
    if uri_path.ends_with("/") {
        &uri_path[..uri_path.len() - 1]
    } else {
        uri_path
    }
}

fn handle_static_path(
    path: &str,
    path_configs: &HashMap<UrlPath, PathConfig>,
    headers: &Option<HashMap<HeaderId, HeaderConfig>>,
) -> Option<Result<Response<Body>, Infallible>> {
    let path_config_hashmap = path_configs.iter().find(|(key, _)| key.as_str() == path);
    let response = match path_config_hashmap {
        Some(path_config_hashmap) => {
            let path_config = path_config_hashmap.1;
            static_path_response(&path_config, headers)
        }
        _ => None,
    };

    match response {
        Some(Ok(response)) => Some(Ok(response)),
        _ => None,
    }
}

fn static_path_response(
    path_config: &PathConfig,
    headers: &Option<HashMap<HeaderId, HeaderConfig>>,
) -> Option<Result<Response<Body>, Error>> {
    if let Some(_) = &path_config.data_src {
        static_path_data_src_reponse(path_config, headers)
    } else {
        if let Some(data_text) = &path_config.data_text {
            Some(
                json_response_base(&path_config.headers, headers)
                    .status(path_config.code)
                    .body(Body::from(data_text.to_owned())),
            )
        } else {
            Some(
                response_base(&path_config.headers, headers)
                    .status(path_config.code)
                    .body(Body::from("")),
            )
        }
    }
}

fn static_path_data_src_reponse(
    path_config: &PathConfig,
    headers: &Option<HashMap<HeaderId, HeaderConfig>>,
) -> Option<Result<Response<Body>, Error>> {
    match std::fs::read_to_string(path_config.data_src.as_ref().unwrap()) {
        Ok(content) => match json5::from_str::<Value>(&content) {
            Ok(json) => {
                let json_text = json.to_string();
                Some(
                    json_response_base(&path_config.headers, headers)
                        .status(path_config.code)
                        .body(Body::from(json_text)),
                )
            }
            _ => Some(
                response_base(&None, &None)
                    .status(StatusCode::BAD_REQUEST)
                    .body(Body::from("Invalid json content")),
            ),
        },
        _ => Some(
            response_base(&None, &None)
                .status(StatusCode::BAD_REQUEST)
                .body(Body::from("Missing json file")),
        ),
    }
}

fn json_response_base(
    path_headers: &Option<Vec<String>>,
    headers: &Option<HashMap<HeaderId, HeaderConfig>>,
) -> Builder {
    response_base(path_headers, headers)
        .header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
}

fn response_base(
    path_headers: &Option<Vec<String>>,
    headers: &Option<HashMap<HeaderId, HeaderConfig>>,
) -> Builder {
    let mut ret = Response::builder()
        .header(ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"))
        .header(ACCESS_CONTROL_ALLOW_HEADERS, HeaderValue::from_static("*"))
        .header(
            ACCESS_CONTROL_ALLOW_METHODS,
            HeaderValue::from_static("GET, POST, OPTIONS"),
        );
    if let Some(path_headers) = path_headers {
        let headers = headers.clone().unwrap();
        for path_header in path_headers {
            let header = headers.get(path_header).unwrap();
            ret = ret.header(header.key.as_str(), header.value.clone().unwrap().as_str());
        }
    }
    ret
}

fn handle_dyn_path(path: &str, dyn_data_dir: &str) -> Option<Result<Response<Body>, Error>> {
    let p = Path::new(dyn_data_dir).join(path.strip_prefix("/").unwrap_or_default());

    let dir = p.parent().unwrap();
    if !Path::new(dir).exists() {
        return Some(Ok(response_base(&None, &None)
            .status(StatusCode::NOT_FOUND)
            .body(Body::empty())
            .unwrap()));
    }

    let file_name = p.file_stem().unwrap();

    let mut json_file = None;
    for entry in fs::read_dir(dir).unwrap() {
        let p = entry.unwrap().path();
        if p.file_stem().unwrap() != file_name {
            continue;
        }

        if p.file_name().unwrap().to_ascii_lowercase() == file_name.to_ascii_lowercase() {
            json_file = Some(p.as_path().to_owned());
            break;
        }
        let ext = p.extension().unwrap_or_default().to_ascii_lowercase();
        if ["json", "json5"].contains(&ext.to_str().unwrap()) {
            json_file = Some(p.as_path().to_owned());
            break;
        }
    }

    let response = match json_file {
        Some(json_file) => match std::fs::read_to_string(json_file) {
            Ok(content) => match json5::from_str::<Value>(&content) {
                Ok(json) => {
                    let body = json.to_string();
                    json_response_base(&None, &None)
                        .status(StatusCode::OK)
                        .body(Body::from(body))
                }
                _ => response_base(&None, &None)
                    .status(StatusCode::BAD_REQUEST)
                    .body(Body::from("Invalid json content")),
            },
            _ => response_base(&None, &None)
                .status(StatusCode::NOT_FOUND)
                .body(Body::empty()),
        },
        _ => response_base(&None, &None)
            .status(StatusCode::NOT_FOUND)
            .body(Body::empty()),
    };

    Some(Ok(response.unwrap()))
}
