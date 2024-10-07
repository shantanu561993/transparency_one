use log_structs::Operator;
use reqwest::Error;
use futures::StreamExt;
use tokio::task::JoinHandle;
use std::{collections::HashMap, env};
mod log_structs;
use std::sync::Arc;
use tokio::sync::Mutex;
mod generic_utils;
mod merkle_tree;

#[derive(serde::Serialize, serde::Deserialize, Debug)]
struct Domain {
    domain: String,
}


async fn send_request(path: &String,debug:bool) -> Result<String, Error> {
    if debug{
        println!("[+] Sending request to: {}", path);
    }
    let response = reqwest::get(path).await?.error_for_status()?;
    let body = response.text().await?;
    if debug{
        println!("[+] Received response from: {}", path);
    }
    Ok(body)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Get command line arguments
    let args: Vec<String> = env::args().collect();
    let mut connection_string = String::new();
    let mut threads = 50 as usize;
    let mut mongodb_name = String::new();
    if args.contains(&String::from("--show")) {
        // Run code for --show flag
        let client_list = get_all_working_operators().await;
        println!("[+] Found {} working operators",client_list.len());
        for (name,url) in client_list.iter(){
            println!("{}: {}",name,url);
        }
    }
    else if args.contains(&String::from("--dump")) {
        let index = args.iter().position(|r| r == "--dump").unwrap();
        let log_name = args[index+1].clone();
        println!("[+] Dumping log: {}",log_name);
        if args.contains(&String::from("--connection-string")) {
            // Run code for --connection-string flag
            let index = args.iter().position(|r| r == "--connection-string").unwrap();
            connection_string = args[index+1].clone();
            println!("[+] Connection String provided: {connection_string}");
        }
        else{
            //exit 
            println!("No Connection flag provided. Please provide --connection-string flag.");
            std::process::exit(1);
        }
        if args.contains(&String::from("--threads")) {
            let index = args.iter().position(|r| r == "--threads").unwrap();
            threads = args[index + 1].clone().parse::<usize>().unwrap();
            println!("[+] Number of Threads : {threads}");
        }
        else{
            println!("[+] Number of threads (default): {threads}");
        }
        if args.contains(&String::from("--mongodb-name")) {
            let index = args.iter().position(|r| r == "--mongodb-name").unwrap();
            mongodb_name = args[index+1].clone();
            println!("[+] Mongodb Name provided: {mongodb_name}");
        }
        else{
            
            println!("[-] --mongodb-name flag not provided. Exiting");
            std::process::exit(1);
        }
        dump_function(log_name,connection_string,threads,mongodb_name).await;

    }
    else{
        println!("Usage: ct-scraper [OPTION] [ARGS]");
        println!("Options:");
        println!("--show: Show all working operators");
        println!("--dump: Dump one operator to a mongodb database");
    }


    // let paths = vec![
    // "https://example.com/?lol=1".to_string(),
    // "https://example.com/?lol=2".to_string(),
    // "https://example.com/?lol=3".to_string(),
    // ];
    // let fetches = futures::stream::iter(
    // paths.into_iter().map(|path| {
    //     async move {
    //         send_request(path).await.unwrap();
    //     }
    // })
    // ).buffer_unordered(100).collect::<Vec<()>>();
    // fetches.await;
    Ok(())
}
pub async fn get_all_working_operators()->HashMap<String,String>{
    let operators: HashMap<String, String> = get_all_operators().await;
    let operators_in_list: Vec<(String,String)> = operators.iter().map(|(key, value)| (key.clone(),value.clone())).collect();
    let mut tasks: Vec<JoinHandle<()>>= vec![];
    let mut return_hashmap:Arc<Mutex<HashMap<String,String>>> = Arc::new(Mutex::new(HashMap::new()));

    for op in &operators_in_list {

        // Copy each path into a new string
        // that can be consumed/captured by the task closure
        let mut op_url = op.1.clone();
        op_url.push_str("ct/v1/get-sth");
        let op_name = op.0.clone();
        let return_hashmap_clone = Arc::clone(&return_hashmap);


        // Create a Tokio task for each path
        tasks.push(tokio::spawn(async move {
            let data = send_request(&op_url,false).await;
            match data{
                Ok(_)=>{
                    let mut map = return_hashmap_clone.lock().await;
                    map.insert(op_name.clone(),op_url.clone());
                },
                Err(_)=>{
                    //println!("[-] Failed to fetch {} log from {}",op_name,op_url)
                },
            }
        }));
    }
    futures::future::join_all(tasks).await;
    let return_hashmap = return_hashmap.lock().await;
    return return_hashmap.clone();
}

async fn get_all_operators()->HashMap<String, String>{
    const ROOT_URL:&str =  "https://www.gstatic.com/ct/log_list/v3/all_logs_list.json";
    let data =  send_request(&ROOT_URL.to_string(),false).await.unwrap_or_else(|error| panic!("Unable to send request. {error}"));
    println!("[+] Parsing root");
    let parsed_root : log_structs::Root = serde_json::from_str(&data).unwrap_or_else(|error| panic!("Unable to parse root {error}"));
    let operators = get_log_sources(parsed_root).await;
    println!("[+] Found {} operators to scan", operators.len());
    return operators;
}
async fn get_log_sources(root: log_structs::Root) -> HashMap<String, String> {
    let mut operators: HashMap<String, String> = HashMap::new();
    for op in root.operators {
        for log in op.logs {
            operators.insert(log.description.to_string(), log.url.to_string());
        }
    }
    return operators;
}

async fn dump_function(log_name:String,connection_string:String,threads:usize,mongodb_name:String){
    println!("[+] Starting DB Connection");
    let mongo_client = mongodb::Client::with_options(mongodb::options::ClientOptions::parse(connection_string).await.unwrap()).unwrap();
    let database = mongo_client.database(&mongodb_name);
    let mongo_collection: Arc<mongodb::Collection<Domain>> = Arc::new(database.collection("domains"));
    let index_model = mongodb::IndexModel::builder()
        .keys(mongodb::bson::doc! { "domain": 1 })
        .options(mongodb::options::IndexOptions::builder().unique(true).build())
        .build();
    mongo_collection.create_index(index_model).await.unwrap_or_else(|_op|panic!("Something went wrong creating index"));
    println!("[+] DB Connection Established");
    let operators: HashMap<String, String> = get_all_operators().await;
    println!("{:?}",operators);
    let log_url = operators.get(&log_name).unwrap_or_else(|| panic!("Log not found"));
    println!("[+] Starting to fetch log: {}",log_name);
    dump_single_log(log_url.to_string(),threads,mongo_collection).await;
}
async fn dump_single_log(log_url:String,threads:usize,mongo_collection:Arc<mongodb::Collection<Domain>>){
    let mut endpoint_url = log_url.clone();
    endpoint_url.push_str("ct/v1/get-sth");
    println!("[+] Sending tree size request to: {}",endpoint_url);
    let data = send_request(&endpoint_url,false).await.unwrap_or_else(|error| panic!("Unable to send request. {error}"));
    let parsed_sth: log_structs::STH = serde_json::from_str(&data).unwrap_or_else(|error| panic!("Unable to parse STH {error}"));
    let tree_size = parsed_sth.tree_size;
    println!("[+] Tree size: {}",tree_size);
    let mut batch_size_request = log_url.clone();
    println!("[+] Getting Batch Size");
    batch_size_request.push_str("ct/v1/get-entries?start=0&end=100000");
    let data = send_request(&batch_size_request,false).await.unwrap_or_else(|error| panic!("Unable to send request. {error}"));
    let parsed_batch_size = generic_utils::read_base64_entries(&data).await.unwrap_or_else(|error| panic!("Unable to parse batch size {error}")).entries.len();
    println!("[+] Batch Size: {}",parsed_batch_size);
    let mut line = String::new();
    println!("Waiting for input :");
    let b1 = std::io::stdin().read_line(&mut line).unwrap();
    let mut start = 0;
    let mut end = 0;
    let mut urls = vec![];
    while end < tree_size{
        end = start + (parsed_batch_size as i64);
        urls.push(format!("{}ct/v1/get-entries?start={}&end={}",log_url,start,end));
        start = end;
    }
    let fetches = futures::stream::iter(
        urls.into_iter().map(|path| {
            let mut data = String::new();
            async move {
                loop{
                    data = send_request(&path,true).await.unwrap_or_else(|_error| "continue".to_string());
                    if data != "continue"{
                        break;
                    }
                    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                    continue;
                }
                let parsed_entries = generic_utils::read_base64_entries(&data).await.unwrap_or_else(|error| generic_utils::Entries{entries:vec![]}); 
                for entry in parsed_entries.entries{
                    let domain_data = merkle_tree::utils::read_entry(&entry).await;
                    println!("{:?}",domain_data);
                    for (key,value) in domain_data{
                        for domain in value{
                            let domain = Domain{domain:domain};
                            println!("[+] Inserting domain: {}",domain.domain);
                            // match mongo_collection.insert_one(domain).await{
                            //     Ok(_)=>{
                            //         println!("[+] Inserted domain: {}",domain.domain);
                            //     },
                            //     Err(_)=>{
                            //         println!("[-] Failed to insert domain: {}",domain.domain);
                            //     }
                            // };
                        }
                    }
                }
            }
        })
    ).buffer_unordered(1).collect::<Vec<()>>();
    fetches.await;
    // let paths = vec![
    // "https://example.com/?lol=1".to_string(),
    // "https://example.com/?lol=2".to_string(),
    // "https://example.com/?lol=3".to_string(),
    // ];
    // let fetches = futures::stream::iter(
    // paths.into_iter().map(|path| {
    //     async move {
    //         send_request(path).await.unwrap();
    //     }
    // })
    // ).buffer_unordered(100).collect::<Vec<()>>();
    // fetches.await;

}