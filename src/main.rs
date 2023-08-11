use std::{path::PathBuf, fmt};
use url::Url;

use anyhow::{anyhow, Result};
use clap::{Parser, Subcommand, ValueEnum};
use dirs::home_dir;
use serde::{Deserialize, Serialize};
use yaml_front_matter::{Document, YamlFrontMatter};

const FILE_NAME: &str = ".markmedium";

/// Publish Medium articles from markdown content
#[derive(Parser)]
#[command(author, version, about, long_about = None, arg_required_else_help(true))]
struct Args {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Set up with your integration token
    Init { token: String },
    /// Publish markdown content on your Medium blog
    Publish { file: PathBuf },
}

#[derive(Debug, Serialize, Deserialize)]
struct MediumUser {
    id: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct MediumUserResponse {
    data: MediumUser,
}

#[derive(Debug, Serialize, Deserialize, ValueEnum, Clone)]
enum PublishStatus {
    #[serde(rename = "public")]
    Public,
    #[serde(rename = "draft")]
    Draft,
    #[serde(rename = "unlisted")]
    Unlisted,
}

impl fmt::Display for PublishStatus {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            PublishStatus::Public => write!(f, "public"),
            PublishStatus::Draft => write!(f, "draft"),
            PublishStatus::Unlisted => write!(f, "unlisted")
        }
    }
}

#[derive(Serialize, Deserialize)]
struct ApiConfig {
    token: String,
    id: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct PublishMetadata {
    title: String,
    #[serde(default)]
    content: String,
    #[serde(rename(serialize = "contentFormat"), default = "default_content_format")]
    content_format: String,
    tags: Option<Vec<String>>,
    #[serde(rename(serialize = "canonicalUrl"))]
    canonical_url: Option<String>,
    #[serde(rename(serialize = "publishStatus"))]
    status: Option<PublishStatus>,
}

fn default_content_format() -> String {
    "markdown".to_string()
}

#[derive(Deserialize)]
struct PublishedPost {
    url: String,
}

#[derive(Deserialize)]
struct PublishResponse {
    data: PublishedPost
}

#[derive(Debug, Serialize, Deserialize)]
struct ErrorBody {
    message: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct ErrorResponse {
    errors: Vec<ErrorBody>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
enum ResponseType<T> {
    Ok(T),
    Err(ErrorResponse),
}

async fn init(token: &String) -> anyhow::Result<PathBuf> {
    let file_path = home_dir().unwrap().join(FILE_NAME);
    let response: reqwest::Response = reqwest::Client::new()
        .get("https://api.medium.com/v1/me")
        .bearer_auth(token)
        .send()
        .await?;

    let response: ResponseType<MediumUserResponse> = response.json().await?;

    match response {
        ResponseType::Ok(user_response) => {
            let user_data = user_response.data;

            let config = ApiConfig {
                token: token.to_string(),
                id: user_data.id,
            };

            let json_config = serde_json::to_string(&config)?;

            std::fs::write(file_path.clone(), json_config)?;
            Ok(file_path)
        }
        ResponseType::Err(error_response) => {
            Err(anyhow!(error_response.errors[0].message.to_owned()))
        }
    }
}

fn read_config() -> Result<ApiConfig> {
    let file_path = home_dir().unwrap().join(FILE_NAME);
    let text: String = std::fs::read_to_string(file_path)?;
    let config: ApiConfig = serde_json::from_str(&text)?;
    Ok(config)
}

fn base_url(mut url: Url) -> Result<Url> {
    match url.path_segments_mut() {
        Ok(mut path) => {
            path.clear();
        }
        Err(_) => {
            return Err(anyhow!("Cannot be a base"));
        }
    }

    url.set_query(None);

    Ok(url)
}

fn get_canonical_reference(canonical_url: String) -> Result<String, anyhow::Error> {
    let url = Url::parse(&canonical_url)?;
    let base = base_url(url.clone())?;
    Ok(
        format!(
            "\n\n---\n\n*Originally published at [{}]({}).*",
            base.as_str().trim_end_matches('/'),
            url
        )
    )
}


async fn publish(mdfile: PathBuf) -> Result<String, anyhow::Error> {
    let config = read_config()?;
    let input = std::fs::read_to_string(mdfile)?;
    let document: Document<PublishMetadata> = YamlFrontMatter::parse::<PublishMetadata>(&input).unwrap();
    let Document { mut metadata, content } = document;

    metadata.content = content;

    if let Some(ref canonical_url) = metadata.canonical_url {
        // Add the "Originally published at XXX"
        metadata.content += get_canonical_reference(canonical_url.to_string())?.as_str();
    }
        

    let response: reqwest::Response = reqwest::Client::new()
        .post(format!(
            "https://api.medium.com/v1/users/{}/posts",
            config.id
        ))
        .bearer_auth(config.token)
        .json(&metadata)
        .send()
        .await?;

    let response: ResponseType<PublishResponse> = response.json().await?;
    
    match response {
        ResponseType::Ok(publish_response) => {
            let publish_data = publish_response.data;
            Ok(publish_data.url)
        }
        ResponseType::Err(error_response) =>  {
            Err(anyhow!(error_response.errors[0].message.to_owned()))
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    match &args.command {
        Some(Commands::Init { token }) => {
            let file_path = init(token).await?;
            println!("Saved token and author ID at {}", file_path.display());
        }
        Some(Commands::Publish { file }) => {
            let url = publish(file.to_owned()).await?;
            println!("Done! Your post has been published at {}", url);
        }
        None => {}
    }

    Ok(())
}
