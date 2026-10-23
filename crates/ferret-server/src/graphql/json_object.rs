//! The `JSONObject` scalar: a free-form JSON object such as a document's `meta`.
//!
//! juniper's scalar derive only resolves to a single scalar value, so the traits are implemented
//! by hand to resolve to a nested object instead. It is output-only: no argument takes it, so
//! parsing one from a query is rejected.

use juniper::{
    ArcStr, BoxFuture, ExecutionResult, Executor, FromInputValue, GraphQLType, GraphQLValue,
    GraphQLValueAsync, InputValue, Object, ParseError, ParseScalarResult, ParseScalarValue,
    Registry, ScalarToken, ScalarValue, Selection, Value,
    macros::reflect::{BaseSubTypes, BaseType, Type, Types, WrappedType, WrappedValue},
    marker::IsOutputType,
    meta::MetaType,
};
use serde_json::{Map, Value as Json};

const NAME: &str = "JSONObject";

pub struct JsonObject(pub Map<String, Json>);

fn to_graphql_object<S: ScalarValue>(fields: &Map<String, Json>) -> Object<S> {
    fields
        .iter()
        .map(|(key, value)| (key.clone(), to_graphql(value)))
        .collect()
}

fn to_graphql<S: ScalarValue>(value: &Json) -> Value<S> {
    match value {
        Json::Null => Value::null(),
        Json::Bool(boolean) => Value::scalar(*boolean),
        // GraphQL's `Int` is 32 bits. Larger integers become floats, which is as exact as a
        // JavaScript client reads them anyway.
        Json::Number(number) => {
            match number.as_i64().and_then(|int| i32::try_from(int).ok()) {
                Some(int) => Value::scalar(int),
                None => Value::scalar(number.as_f64().expect(
                    "serde_json without arbitrary precision represents every number as f64",
                )),
            }
        }
        Json::String(string) => Value::scalar(string.clone()),
        Json::Array(items) => Value::list(items.iter().map(to_graphql).collect()),
        Json::Object(fields) => Value::object(to_graphql_object(fields)),
    }
}

impl<S: ScalarValue> GraphQLType<S> for JsonObject {
    fn name(_: &()) -> Option<ArcStr> {
        Some(ArcStr::from(NAME))
    }

    fn meta(info: &(), registry: &mut Registry<S>) -> MetaType<S> {
        registry
            .build_scalar_type::<Self>(info)
            .description("A free-form JSON object.")
            .into_meta()
    }
}

impl<S: ScalarValue> GraphQLValue<S> for JsonObject {
    type Context = ();
    type TypeInfo = ();

    fn type_name(&self, info: &()) -> Option<ArcStr> {
        <Self as GraphQLType<S>>::name(info)
    }

    fn resolve(
        &self,
        _: &(),
        _: Option<&[Selection<S>]>,
        _: &Executor<(), S>,
    ) -> ExecutionResult<S> {
        Ok(Value::object(to_graphql_object(&self.0)))
    }
}

impl<S: ScalarValue + Send + Sync> GraphQLValueAsync<S> for JsonObject {
    fn resolve_async<'a>(
        &'a self,
        info: &'a (),
        selection_set: Option<&'a [Selection<S>]>,
        executor: &'a Executor<(), S>,
    ) -> BoxFuture<'a, ExecutionResult<S>> {
        Box::pin(std::future::ready(self.resolve(
            info,
            selection_set,
            executor,
        )))
    }
}

impl<S: ScalarValue> IsOutputType<S> for JsonObject {}

impl<S> FromInputValue<S> for JsonObject {
    type Error = &'static str;

    fn from_input_value(_: &InputValue<S>) -> Result<Self, Self::Error> {
        Err("JSONObject is output-only")
    }
}

impl<S> ParseScalarValue<S> for JsonObject {
    fn from_str(_: ScalarToken<'_>) -> ParseScalarResult<S> {
        Err(ParseError::ExpectedScalarError("JSONObject is output-only"))
    }
}

impl<S> BaseType<S> for JsonObject {
    const NAME: Type = NAME;
}

impl<S> BaseSubTypes<S> for JsonObject {
    const NAMES: Types = &[NAME];
}

impl<S> WrappedType<S> for JsonObject {
    const VALUE: WrappedValue = 1;
}
