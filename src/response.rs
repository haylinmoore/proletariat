use crate::v8gen;

fn constructor(
    scope: &mut v8::HandleScope<'_>,
    args: v8::FunctionCallbackArguments<'_>,
    mut retval: v8::ReturnValue<'_>,
) {
    let null = v8::null(scope);

    let values_body: v8::Local<'_, v8::Value>;
    if args.get(0) != null || args.get(0).is_string() {
            values_body = args.get(0);

    } else {
        values_body = v8gen::generate_v8_string(scope, "No body provided").into(); 
    }

    let names_body = v8gen::generate_v8_string(scope, "body");
    retval.set(v8::Object::with_prototype_and_properties(scope, null.into(), &[names_body.into()], &[values_body.into()]).into());
}

pub fn generate<'a>(
    scope: &mut v8::HandleScope<'a>
) -> v8::Local<'a, v8::FunctionTemplate> {
    let func = v8::FunctionTemplate::new(scope, constructor);

    return func;
}
