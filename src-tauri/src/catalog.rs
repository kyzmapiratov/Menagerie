// Catalog source: shimejis.xyz (the Shimeji Directory).
//
// Why this one: ~60 packs by franchise and hundreds of characters, and above
// all every character has a real sprite at a predictable address:
//
//   https://sprites.shimejis.xyz/directory/{slug}/img/shime1.png
//
// Thanks to that the catalog shows the character itself rather than a page
// thumbnail, and hover previews can animate by cycling a few frames.
//
// We parse by link structure (a[href*="/directory/"]), not by class names, so
// the scraper survives a change in the site's markup.

use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};

const SITE: &str = "https://shimejis.xyz";
const SPRITES: &str = "https://sprites.shimejis.xyz";

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Pack {
    /// slug like "pokemon-shimeji-pack"
    pub slug: String,
    pub title: String,
    /// a few sprites of this pack's characters, for the cover preview
    pub preview_sprites: Vec<String>,
    pub character_count: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Character {
    /// slug like "pokemon-eevee-by-cachomon"
    pub slug: String,
    pub name: String,
    /// main sprite (shime1.png)
    pub sprite: String,
    /// frames for the hover animation
    pub frames: Vec<String>,
    pub pack_slug: String,
    pub pack_title: String,
}

pub fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(concat!("menagerie/", env!("CARGO_PKG_VERSION"), " (+https://github.com/kyzmapiratov/Menagerie)"))
        .build()
        .expect("could not create the HTTP client")
}

/// Base address of one character's sprites.
pub fn sprite_base(slug: &str) -> String {
    format!("{SPRITES}/directory/{slug}/img")
}

fn sprite_url(slug: &str, n: usize) -> String {
    format!("{}/shime{n}.png", sprite_base(slug))
}

/// Frames for the hover animation.
///
/// In the classic Shimeji-ee set spr1 is standing and the following frames are
/// walking phases. We take the first few: if a frame does not exist, the <img>
/// in the frontend simply skips it.
fn preview_frames(slug: &str) -> Vec<String> {
    (1..=4).map(|n| sprite_url(slug, n)).collect()
}

async fn get_html(url: &str) -> Result<Html, String> {
    let body = client()
        .get(url)
        .send()
        .await
        .map_err(|e| format!("could not load {url}: {e}"))?
        .text()
        .await
        .map_err(|e| format!("could not read the response: {e}"))?;
    Ok(Html::parse_document(&body))
}

/// List of packs (franchises) from the catalog home page.
pub async fn fetch_packs() -> Result<Vec<Pack>, String> {
    let doc = get_html(&format!("{SITE}/directory")).await?;
    let link_sel = Selector::parse("a[href*=\"-shimeji-pack\"]").unwrap();
    let img_sel = Selector::parse("img").unwrap();

    let mut packs: Vec<Pack> = Vec::new();

    for a in doc.select(&link_sel) {
        let Some(href) = a.value().attr("href") else {
            continue;
        };
        let Some(slug) = href.rsplit('/').next() else {
            continue;
        };
        if !slug.ends_with("-shimeji-pack") || packs.iter().any(|p| p.slug == slug) {
            continue;
        }

        // The link text is the name plus the word "shimeji" repeated once per image
        // inside it. Cut everything from the first occurrence.
        let raw = a.text().collect::<String>();
        let title = raw
            .split("shimeji")
            .next()
            .unwrap_or(&raw)
            .trim()
            .to_string();

        // Character thumbnails inside the link are used as the pack preview.
        let sprites: Vec<String> = a
            .select(&img_sel)
            .filter_map(|img| {
                img.value()
                    .attr("src")
                    .or_else(|| img.value().attr("data-src"))
                    .map(absolutize)
            })
            .collect();

        let count = sprites.len();

        packs.push(Pack {
            slug: slug.to_string(),
            title: if title.is_empty() {
                slug.replace("-shimeji-pack", "").replace('-', " ")
            } else {
                title
            },
            preview_sprites: sprites.into_iter().take(4).collect(),
            character_count: count,
        });
    }

    if packs.is_empty() {
        return Err("could not parse the pack list; the site layout may have changed".into());
    }

    Ok(packs)
}

/// Characters of one pack.
pub async fn fetch_characters(pack_slug: &str) -> Result<Vec<Character>, String> {
    let url = format!("{SITE}/directory/{pack_slug}");
    let doc = get_html(&url).await?;

    let link_sel = Selector::parse("a[href*=\"/directory/shimeji/\"]").unwrap();
    let img_sel = Selector::parse("img").unwrap();
    let h1_sel = Selector::parse("h1").unwrap();

    let pack_title = doc
        .select(&h1_sel)
        .next()
        .map(|h| {
            h.text()
                .collect::<String>()
                .trim()
                .trim_end_matches(" shimejis")
                .to_string()
        })
        .unwrap_or_else(|| pack_slug.replace("-shimeji-pack", "").replace('-', " "));

    let mut chars: Vec<Character> = Vec::new();

    for a in doc.select(&link_sel) {
        let Some(href) = a.value().attr("href") else {
            continue;
        };
        // Skip service links like .../activate
        if href.ends_with("/activate") {
            continue;
        }
        let Some(slug) = href.rsplit('/').next() else {
            continue;
        };
        if slug.is_empty() || chars.iter().any(|c| c.slug == slug) {
            continue;
        }

        // The character name is the link text without the images' alt texts.
        let name = a
            .text()
            .collect::<String>()
            .replace("shimeji", "")
            .trim()
            .to_string();
        if name.is_empty() {
            continue;
        }

        // Take the sprite from the page; if it is missing, construct the URL.
        let sprite = a
            .select(&img_sel)
            .next()
            .and_then(|img| img.value().attr("src").map(absolutize))
            .unwrap_or_else(|| sprite_url(slug, 1));

        chars.push(Character {
            frames: preview_frames(slug),
            slug: slug.to_string(),
            name,
            sprite,
            pack_slug: pack_slug.to_string(),
            pack_title: pack_title.clone(),
        });
    }

    Ok(chars)
}

/// A character's artist (from its page). Optional: only for the details card.
pub async fn fetch_artist(slug: &str) -> Option<String> {
    let doc = get_html(&format!("{SITE}/directory/shimeji/{slug}")).await.ok()?;
    let sel = Selector::parse("a[href*=\"deviantart\"], a[href*=\"artist\"]").ok()?;
    doc.select(&sel)
        .next()
        .map(|a| a.text().collect::<String>().trim().to_string())
        .filter(|s| !s.is_empty())
}

fn absolutize(src: &str) -> String {
    if src.starts_with("http") {
        src.to_string()
    } else if src.starts_with("//") {
        format!("https:{src}")
    } else if src.starts_with('/') {
        format!("{SITE}{src}")
    } else {
        format!("{SITE}/{src}")
    }
}

// ---------------------------------------------------------------------------
// Global index for searching the whole catalog
// ---------------------------------------------------------------------------

use futures::stream::{self, StreamExt};

/// Collects characters from all packs. There are many requests (~60), so we run
/// them a few at a time in parallel; sequentially it would take half a minute.
pub async fn build_index() -> Result<Vec<Character>, String> {
    let packs = fetch_packs().await?;

    let results: Vec<Vec<Character>> = stream::iter(packs.into_iter().map(|p| async move {
        fetch_characters(&p.slug).await.unwrap_or_default()
    }))
    .buffer_unordered(6)
    .collect()
    .await;

    let mut all: Vec<Character> = results.into_iter().flatten().collect();
    all.sort_by_key(|c| c.name.to_lowercase());
    Ok(all)
}
