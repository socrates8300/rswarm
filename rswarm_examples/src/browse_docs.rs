// use std::error::Error;
use headless_chrome::Browser;
// use headless_chrome::protocol::cdp::Page;
use anyhow::Result;
use rswarm::types::{ContextVariables, ResultType};
use html_escape::decode_html_entities;

pub fn browse_rust_docs(args: ContextVariables) -> Result<ResultType> {
    // Extract the 'query' argument from ContextVariables
    let query = args
        .get("query")
        .ok_or_else(|| anyhow::anyhow!("Argument 'query' is required."))?
        .clone();

    let browser = Browser::default()?;
    let tab = browser.new_tab()?;

    // Navigate directly to the crate's documentation page
    let docs_url = format!("https://docs.rs/{}/latest/{}", query, query);
    println!("Navigating to URL: {}", &docs_url);
    tab.navigate_to(&docs_url)?;
    tab.wait_until_navigated()?;

    // Initialize the extracted_info with the crate name
    let mut extracted_info = format!("Crate: {}\n", query);

    // Define the selector for re-exports
    let reexports_selector = "div.item-name[id^='reexport.'] a";
    println!("Searching for re-exports using selector: {}", reexports_selector);

    // Find all re-export elements
    let reexport_elements = tab.find_elements(reexports_selector)?;

    // Debug: Print the number of re-export elements found
    println!("Found {} re-export elements.", reexport_elements.len());

    // Collect all hrefs first to avoid navigating mid-iteration
    let mut hrefs: Vec<String> = Vec::new();

    for element in &reexport_elements {
        if let Some(attributes) = element.get_attributes()? {
            // Attributes are in [name1, value1, name2, value2, ...] format
            for chunk in attributes.chunks(2) {
                if chunk.len() == 2 && chunk[0] == "href" {
                    let href = chunk[1].to_string();
                    println!("Collected href: {}", href); // Debug log
                    hrefs.push(href);
                }
            }
        }
    }

    println!("Total hrefs collected: {}", hrefs.len());

    // Iterate over each href and extract HTML content
    for href in hrefs {
        // Construct the full URL
        let full_url = format!("https://docs.rs/{}/latest/{}", query, href);
        println!("Navigating to re-export URL: {}", &full_url);

        // Navigate to the re-export URL
        tab.navigate_to(&full_url)?;
        tab.wait_until_navigated()?;

        // Retrieve the page's HTML content
        let html_content = tab.get_content()?;
        println!("Retrieved HTML content from: {}", &full_url);

        // Append the HTML content to extracted_info with separators for clarity
        extracted_info.push_str(&format!(
            "\n\n<!-- Start of {} -->\n{}\n<!-- End of {} -->\n",
            href, html_content, href
        ));
    }

    println!("Extracted info:\n{}", &extracted_info);

    // Return the extracted information
    Ok(ResultType::Value(clean_up_extracted_info(extracted_info)))
}

fn clean_up_extracted_info(extracted_info: String) -> String {
    // Decode HTML entities
    let decoded = decode_html_entities(&extracted_info);

    // Split into lines, trim each line, and filter out empty lines
    decoded
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}