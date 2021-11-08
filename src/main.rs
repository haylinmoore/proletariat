#![forbid(rust_2018_idioms)]
use v8;
use std::time::Duration;
use std::convert::Infallible;
use std::sync::mpsc::{Sender, Receiver};
use std::sync::mpsc;
use hyper::{Server};
use hyper::service::{make_service_fn, service_fn};

mod v8gen;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    //!
    let platform = v8::new_default_platform(0, false).make_shared();
    v8::V8::initialize_platform(platform);
    v8::V8::initialize();

    // For every connection, we must make a `Service` to handle all
    // incoming HTTP requests on said connection.
    let make_svc = make_service_fn(|_conn| {
        // This is the `Service` that will handle the connection.
        // `service_fn` is a helper to convert a function that
        // returns a Response into a `Service`.
        async { Ok::<_, Infallible>(service_fn(handle_request)) }
    });

    let addr = ([127, 0, 0, 1], 3000).into();

    let server = Server::bind(&addr).serve(make_svc);

    println!("Listening on http://{}", addr);

    server.await?;
    Ok(())
}

async fn handle_request(mut hyper_req: hyper::Request<hyper::Body>) -> Result<hyper::Response<hyper::Body>, Infallible> {

    let headers = hyper_req.headers_mut();
    let code: String;
    if !headers.contains_key("host"){
        headers.insert("host", "".parse().unwrap());
    }

    let mut host = headers.get("host").unwrap().to_str().unwrap().split(":");
    code = match host.nth(0).unwrap() {
        "localhost4" => String::from("
async function generateResponse(request){
return new Response(`Hello ${request.url} from a Promise!`);
}
async function handleRequest(request){
return await generateResponse(request); 
}

addEventListener('fetch', event => {
return handleRequest(event.request); 
});
"),
        _ => String::from("
addEventListener('fetch', event => {
return new Response('Unknown host header'); 
});
        ")
    };

    let receiver = v8gen::create_v8_environment(hyper_req, code);
    
    let response = receiver.recv_timeout(Duration::from_secs(5));

    match response {
        Ok(data) => {
            return Ok(hyper::Response::new(hyper::Body::from(data)));
        }
        Err(_) => {
            return Ok(hyper::Response::new(hyper::Body::from("Execution timeout reached")));
        }
    }
}
