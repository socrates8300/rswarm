use anyhow::{anyhow, Context, Result};
use headless_chrome::Browser;
use html_escape::decode_html_entities;
use rswarm::types::{ContextVariables, ResultType};
use scraper::{Html, Selector};

pub fn browse_rust_docs(args: ContextVariables) -> Result<ResultType> {
    let query = args
        .get("query")
        .ok_or_else(|| anyhow!("Argument 'query' is required."))?
        .clone();

    let browser = Browser::default().context("Failed to initialize headless Chrome browser")?;
    let tab = browser.new_tab().context("Failed to open a new tab")?;

    let docs_url = format!("https://docs.rs/{}/latest/{}", query, query);
    println!("Navigating to URL: {}", docs_url);
    navigate_to_url(&tab, &docs_url)?;

    let mut extracted_info = format!("Crate: {}\n", query);

    let reexports_selector = "div.item-name[id^='reexport.'] a";
    println!("Searching for re-exports using selector: {}", reexports_selector);

    let reexport_hrefs = collect_href_attributes(&tab, reexports_selector)?;
    println!("Collected {} hrefs for re-exports.", reexport_hrefs.len());

    for href in reexport_hrefs {
        let full_url = format!("https://docs.rs/{}/latest/{}/{}", query, query, href);
        println!("Navigating to re-export URL: {}", full_url);

        navigate_to_url(&tab, &full_url)?;
        let html_content = tab
            .get_content()
            .context(format!("Failed to retrieve content from URL: {}", full_url))?;
        println!("Retrieved HTML content from: {}", full_url);

        extracted_info.push_str(&format!(
            "\n\n<!-- Start of {} -->\n{}\n<!-- End of {} -->\n",
            href, html_content, href
        ));
    }

    let cleaned_info = clean_up_extracted_info(extracted_info);
    // println!("Extracted info:\n{}", &cleaned_info);

    Ok(ResultType::Value(cleaned_info))
}

/// Navigates to the specified URL and ensures the page is fully loaded.
fn navigate_to_url(tab: &headless_chrome::Tab, url: &str) -> Result<()> {
    tab.navigate_to(url)
        .context(format!("Failed to navigate to URL: {}", url))?;
    tab.wait_until_navigated()
        .context(format!("Failed to wait for navigation to complete: {}", url))?;
    Ok(())
}

/// Collects all href attributes matching the given selector.
fn collect_href_attributes(tab: &headless_chrome::Tab, selector: &str) -> Result<Vec<String>> {
    let elements = tab
        .find_elements(selector)
        .context(format!("Failed to find elements with selector: {}", selector))?;

    let mut hrefs = Vec::new();
    for element in &elements {
        if let Some(attributes) = element.get_attributes()? {
            hrefs.extend(
                attributes
                    .chunks(2)
                    .filter(|chunk| chunk.len() == 2 && chunk[0] == "href")
                    .map(|chunk| chunk[1].to_string()),
            );
        }
    }
    Ok(hrefs)
}

/// Cleans up and extracts meaningful text from raw HTML content.
fn clean_up_extracted_info(extracted_info: String) -> String {
    let decoded = decode_html_entities(&extracted_info);
    let document = Html::parse_document(&decoded);

    let selector = Selector::parse("main, p, h1, h2, h3, h4, h5, h6, li").unwrap();
    document
        .select(&selector)
        .map(|element| element.text().collect::<Vec<_>>().join(" ").trim().to_string())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}