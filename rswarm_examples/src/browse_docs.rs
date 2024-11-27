// use std::error::Error;
use headless_chrome::Browser;
// use headless_chrome::protocol::cdp::Page;
use anyhow::Result;
use rswarm::types::{ContextVariables, ResultType};

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
    tab.navigate_to(&docs_url)?;
    tab.wait_until_navigated()?;

    // Update the extracted info to use the query as the crate name
    let mut extracted_info = format!("Crate: {}\n", query);

    // Navigate to the 'Structs' section if it exists
    let structs_selector = "a#structs";
    let structs_link = tab.wait_for_element(structs_selector);

    if let Ok(link) = structs_link {
        // Click on the 'Structs' section
        link.click()?;
        tab.wait_until_navigated()?;

        // Extract the list of structs
        let struct_items_selector = "h2#structs + ul.item-table > li > div.item-name > a.struct";
        let struct_items = tab.wait_for_elements(struct_items_selector)?;

        if !struct_items.is_empty() {
            extracted_info.push_str("Structs:\n");
            for item in struct_items {
                let struct_name = item.get_inner_text()?;
                extracted_info.push_str(&format!("- {}\n", struct_name));
            }
        }
    }

    // Similarly, extract Enums, Traits, Functions as needed
    // For example, extracting functions:
    let functions_selector = "a#functions";
    let functions_link = tab.wait_for_element(functions_selector);

    if let Ok(link) = functions_link {
        // Click on the 'Functions' section
        link.click()?;
        tab.wait_until_navigated()?;

        // Extract the list of functions
        let function_items_selector = "h2#functions + ul.item-table > li > div.item-name > a.fn";
        let function_items = tab.wait_for_elements(function_items_selector)?;

        if !function_items.is_empty() {
            extracted_info.push_str("Functions:\n");
            for item in function_items {
                let function_name = item.get_inner_text()?;
                extracted_info.push_str(&format!("- {}\n", function_name));
            }
        }
    }

    // Return the extracted information
    Ok(ResultType::Value(extracted_info))
}
