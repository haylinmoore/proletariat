use v8;
use std::sync::mpsc::{Sender, Receiver};
use std::sync::mpsc;

pub fn generate_v8_string<'a>(
    scope: &mut v8::HandleScope<'a>,
    str: &str 
) -> v8::Local<'a, v8::String> {
    return v8::String::new(scope,str).unwrap();
}

pub fn generate_v8_int<'a>(
    scope: &mut v8::HandleScope<'a>,
    num: i32 
) -> v8::Local<'a, v8::Integer> {
    return v8::Integer::new(scope,num);
}

fn create_v8_request_object<'a>(
    scope: &mut v8::HandleScope<'a>,
    req: &hyper::Request<()>
) -> v8::Local<'a, v8::Object> {
    let null = v8::null(scope);
    let values_url = v8::String::new(scope,req.uri().path()).unwrap();
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

fn hyper_request_cloner(
    hyper_req: &hyper::Request<hyper::Body>,
) -> hyper::Request<()> {
    let (method, uri, version, headers, _extenstions) = (hyper_req.method().clone(), hyper_req.uri().clone(), hyper_req.version().clone(), hyper_req.headers().clone(), http::Extensions::new());
    let mut req = hyper::Request::builder().uri(uri).version(version).method(method).body(()).unwrap();
    let head = req.headers_mut();
    headers.iter().for_each(
        |(k,v)| {
            head.insert(k,v.clone()); 
            ()
        }
    );

    return req;
}

fn add_event_listener(
    scope: &mut v8::HandleScope<'_>,
    args: v8::FunctionCallbackArguments<'_>,
    mut _retval: v8::ReturnValue<'_>,
) {
    let req = scope.get_slot::<hyper::Request<hyper::Body>>().unwrap().clone();
    let req = hyper_request_cloner(&req);
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

pub fn object_extract_item<'a>(
    scope: &mut v8::HandleScope<'a>,
    obj: v8::Local<'_, v8::Object>,
    name: &str 
) -> v8::Local<'a, v8::Value> {
    let name = v8::String::new(scope,name).unwrap();
    return obj.get(scope, name.into()).unwrap();
}

fn send_response_object(scope: &mut v8::HandleScope<'_>, obj: v8::Local<'_, v8::Object>) {
    let null = v8::null(scope);
    let user_response = object_extract_item(scope, obj, "body").to_string(scope).unwrap().to_rust_string_lossy(scope);
    let body = hyper::Body::from(user_response);
    let mut response = hyper::Response::new(body);
    {
        let headers = object_extract_item(scope, obj, "headers").to_object(scope).unwrap(); 
        let entries = v8::Local::<v8::Function>::try_from(object_extract_item(scope, headers, "entries")).unwrap();
        let result = entries.call(scope, headers.into(), &[]).unwrap().to_object(scope).unwrap();
        let header_vals = v8::Local::<v8::Array>::try_from(object_extract_item(scope, result, "items")).unwrap();
        let heads = response.headers_mut();

        let mut i = 0;
        let mut finished = false;
        loop {
            let val = header_vals.get_index(scope, i).unwrap();
            if (val.is_undefined()){
                break;
            } // End of array
            let val = val.to_object(scope).unwrap();
            let key = val.get_index(scope, 0).unwrap().to_rust_string_lossy(scope);
            let value = val.get_index(scope, 1).unwrap().to_rust_string_lossy(scope);
            heads.insert(hyper::header::HeaderName::from_lowercase(&key.as_bytes()).unwrap(), http::HeaderValue::from_str(&value).unwrap());
            i+=1;
        }

    }
    scope.get_slot::<Sender<hyper::Response<hyper::Body>>>().unwrap().send(
        response
    ).unwrap();
}

pub fn create_v8_environment(
    hyper_req: hyper::Request<hyper::Body>,
    user_code: String
) -> Receiver<hyper::Response<hyper::Body>> {

    let isolate = &mut v8::Isolate::new(Default::default());
    let scope = &mut v8::HandleScope::new(isolate);
    
    scope.set_slot(hyper_req);

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

    // let response_class = response::generate(scope);
    // myglobals.set( 
    //     generate_v8_string(scope, "Response").into(), 
    //     response_class.into()
    // );

    let prepended_js ="
        class Request {
            constructor(url, options){
                this.url = url;
                this.options = options || {};
            }
        }

        function iteratorFor(items) {
            var iterator = {
                next: function () {
                    var value = items.shift();
                    return { done: value === undefined, value: value };
                },
                items: items
            };
        
            return iterator;
        }

        class Headers {
            constructor(headers){
                this.map = {};
                if (headers instanceof Headers) {
                    headers.forEach(function (value, name) {
                        this.append(name, value);
                    }, this);
                } else if (Array.isArray(headers)) {
                    headers.forEach(function (header) {
                        this.append(header[0], header[1]);
                    }, this);
                } else if (headers) {
                    Object.getOwnPropertyNames(headers).forEach(function (name) {
                        this.append(name, headers[name]);
                    }, this);
                }
            }

            append(name, value) {
                name = this.normalizeName(name);
                value = this.normalizeValue(value);
                var oldValue = this.map[name];
                this.map[name] = oldValue ? oldValue + ', ' + value : value;
            } 

            delete(name){
                delete this.map[this.normalizeName(name)];
            }

            get(name) {
                name = this.normalizeName(name);
                return this.has(name) ? this.map[name] : null;
            }

            has(name) {
                return this.map.hasOwnProperty(this.normalizeName(name));
            }

            set(name, value) {
                this.map[this.normalizeName(name)] = this.normalizeValue(value);
            }

            forEach(callback, thisArg) {
                for (var name in this.map) {
                    if (this.map.hasOwnProperty(name)) {
                        callback.call(thisArg, this.map[name], name, this);
                    }
                }
            }

            keys() {
                var items = [];
                this.forEach(function (value, name) {
                    items.push(name);
                });
                return iteratorFor(items);
            };

            values() {
                var items = [];
                this.forEach(function (value) {
                    items.push(value);
                });
                return iteratorFor(items);
            };

            entries() {
                var items = [];
                this.forEach(function (value, name) {
                    items.push([name, value]);
                });
                return iteratorFor(items);
            };

            normalizeValue(value) {
                if (typeof value !== 'string') {
                    value = String(value);
                }
                return value;
            }

            normalizeName(name) {
                if (typeof name !== 'string') {
                    name = String(name);
                }
                if (/[^a-z0-9\\-#$%&'*+.^_`|~!]/i.test(name) || name === '') {
                    throw new TypeError('Invalid character in header field name: \"' + name + '\"');
                }
                return name.toLowerCase();
            }
        }

        class Response {
            constructor(body, options){
                if (options == undefined) {
                    options = {};
                }
                this.type = 'default'
                this.status = options.status === undefined ? 200 : options.status
                this.ok = this.status >= 200 && this.status < 300
                this.statusText = options.statusText === undefined ? '' : '' + options.statusText
                this.url = options.url || ''
                this.headers = new Headers(options.headers || {});
                this.body = body;
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
    let (sender, receiver): (Sender<hyper::Response<hyper::Body>>, Receiver<hyper::Response<hyper::Body>>) = mpsc::channel();
    scope.set_slot(sender);

    let code = v8::String::new(scope, &format!("{}{}", prepended_js, user_code).to_owned()).unwrap();

    let script = v8::Script::compile(scope, code, None).unwrap();
    script.run(scope).unwrap();

    return receiver;
}