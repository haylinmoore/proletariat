#![forbid(rust_2018_idioms)]
use v8;
use std::time::Duration;
use std::convert::Infallible;
use std::sync::mpsc::{Sender, Receiver};
use std::sync::mpsc;
use hyper::{Server};
use hyper::service::{make_service_fn, service_fn};

#[derive(Clone)]
struct Request {
    path: String,
    body: Option<String>
}

fn generate_v8_string<'a>(
    scope: &mut v8::HandleScope<'a>,
    str: &str 
) -> v8::Local<'a, v8::String> {
    return v8::String::new(scope,str).unwrap();
}

fn create_v8_request_object<'a>(
    scope: &mut v8::HandleScope<'a>,
    req: &Request
) -> v8::Local<'a, v8::Object> {
    let null = v8::null(scope);
    let values_url = v8::String::new(scope,&req.path.clone()).unwrap();
    let names_url = generate_v8_string(scope, "url"); 

    return v8::Object::with_prototype_and_properties(scope, null.into(), &[names_url.into()], &[values_url.into()]);
}

fn prom_response(
    scope: &mut v8::HandleScope<'_>,
    args: v8::FunctionCallbackArguments<'_>,
    mut _retval: v8::ReturnValue<'_>,
) {
    if args.get(0).is_object() {
        get_response(scope, Option::from(args.get(0)));
    }
}
fn get_response(
    scope: &mut v8::HandleScope<'_>,
    res: Option<v8::Local<'_, v8::Value>>,
) {
    match res {
        Some(d) => {
            if d.is_promise() {
                let prom = v8::Local::<v8::Promise>::try_from(d).unwrap();
                let prom_then = v8::Function::new(scope, prom_response).unwrap(); 
                prom.then(scope, prom_then);
            }
            else if d.is_object() {
                let resp = d.to_object(scope).unwrap();
                send_response_object(scope, resp);
            }

        }
        None => {
            print!("Code failed to execute.");
        }
    } 
}

fn add_event_listener(
    scope: &mut v8::HandleScope<'_>,
    args: v8::FunctionCallbackArguments<'_>,
    mut _retval: v8::ReturnValue<'_>,
) {
    let req = scope.get_slot::<Request>().unwrap().clone();
    if args.get(1).is_function() {
        let func_obj = args.get(1).to_object(scope).unwrap();
        let func = v8::Local::<v8::Function>::try_from(func_obj).unwrap();

        let null = v8::null(scope);
        // Request Object
        // Base Object
        let values_type = generate_v8_string(scope, "fetch").into(); 
        let names_type = generate_v8_string(scope, "type").into();
        let names_request = generate_v8_string(scope,"request").into();
        let values_request = create_v8_request_object(scope, &req);

        let args = [v8::Object::with_prototype_and_properties(scope, null.into(), &[names_type, names_request], &[values_type, values_request.into()]).into()];

        let response = func.call(scope, null.into(), &args);
        get_response(scope, response);
    }
}

fn send_response_object(scope: &mut v8::HandleScope<'_>, obj: v8::Local<'_, v8::Object>) {
    let mut user_response: String = String::from("No response given.");
    let body_string = v8::String::new(scope, "body").unwrap();
    let body = obj.get(scope, body_string.into());
    match body {
        Some(s) => {
            user_response = s.to_rust_string_lossy(scope);
        }
        None => {

        }
    }
    scope.get_slot::<Sender<String>>().unwrap().send(
        user_response 
    ).unwrap();
}

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

async fn handle_request(h_req: hyper::Request<hyper::Body>) -> Result<hyper::Response<hyper::Body>, Infallible> {
    let isolate = &mut v8::Isolate::new(Default::default());
    let scope = &mut v8::HandleScope::new(isolate);
    let req = Request {
        path: h_req.uri().path().to_owned().clone(),
        body: None
    };

    scope.set_slot(req.clone());

    let context = v8::Context::new(scope);
    let scope = &mut v8::ContextScope::new(scope, context);
	// first make an "object template" - defining a capability to instance a javascript object such as a "const obj = {}"
	let myglobals = v8::ObjectTemplate::new(scope);

	// variable instances can be added to the somewhat abstract object template - but cannot be read back out so easily
	let req = v8::ObjectTemplate::new(scope);
	req.set( v8::String::new(scope,"path").unwrap().into(), v8::String::new(scope,"/index.html").unwrap().into());

    myglobals.set( 
        v8::String::new(scope, "req").unwrap().into(), 
        req.into()
    );

    let prepended_js ="
        class Response {
            constructor(body){
                this.body = body || '';
            }
        }
    ";


    let event_listener = v8::FunctionTemplate::new(scope, add_event_listener);
    myglobals.set(
        v8::String::new(scope, "addEventListener").unwrap().into(), 
        event_listener.into()
    );

	// there is a convenient concept of an internal; but you do have to pre-allocate the number of slots
	// https://stackoverflow.com/questions/16600735/what-is-an-internal-field-count-and-what-is-setinternalfieldcount-used-for
	// https://v8.dev/docs/embed
	myglobals.set_internal_field_count(1);

	// there is a bit of promotion of this object to become the global scope
	let context = v8::Context::new_from_template(scope, myglobals);
    let scope = &mut v8::ContextScope::new(scope, context);
    let (sender, receiver): (Sender<String>, Receiver<String>) = mpsc::channel();
    scope.set_slot(sender);

    let user_code = String::from("
    async function generateResponse(request){
        return new Response(`Hello ${request.url} from a Promise!`);
    }
    async function handleRequest(request){
        return await generateResponse(request); 
    }

    addEventListener('fetch', event => {
        return handleRequest(event.request); 
    });
    ");

    let code = v8::String::new(scope, &format!("{}{}", prepended_js, user_code).to_owned()).unwrap();

    let script = v8::Script::compile(scope, code, None).unwrap();
    script.run(scope).unwrap();

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
