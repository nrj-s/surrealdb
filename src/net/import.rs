use super::headers::Accept;
use super::AppState;
use crate::cnf::HTTP_MAX_IMPORT_BODY_SIZE;
use crate::err::Error;
use crate::net::input::bytes_to_utf8;
use crate::net::output;
use axum::extract::DefaultBodyLimit;
use axum::response::IntoResponse;
use axum::routing::post;
use axum::Extension;
use axum::Router;
use axum_extra::TypedHeader;
use bytes::Bytes;
use surrealdb::dbs::capabilities::RouteTarget;
use surrealdb::dbs::Session;
use surrealdb::iam::Action::Edit;
use surrealdb::iam::ResourceKind::Any;
use tower_http::limit::RequestBodyLimitLayer;

pub(super) fn router<S>() -> Router<S>
where
	S: Clone + Send + Sync + 'static,
{
	Router::new()
		.route("/import", post(handler))
		.route_layer(DefaultBodyLimit::disable())
		.layer(RequestBodyLimitLayer::new(*HTTP_MAX_IMPORT_BODY_SIZE))
}

async fn handler(
	Extension(state): Extension<AppState>,
	Extension(session): Extension<Session>,
	accept: Option<TypedHeader<Accept>>,
	sql: Bytes,
) -> Result<impl IntoResponse, impl IntoResponse> {
	// Get the datastore reference
	let db = &state.datastore;
	// Check if capabilities allow querying the requested HTTP route
	if !db.allows_http_route(&RouteTarget::Import) {
		warn!("Capabilities denied HTTP route request attempt, target: '{}'", &RouteTarget::Import);
		return Err(Error::ForbiddenRoute(RouteTarget::Import.to_string()));
	}
	// Convert the body to a byte slice
	let sql = bytes_to_utf8(&sql)?;
	// Check the permissions level
	db.check(&session, Edit, Any.on_level(session.au.level().to_owned()))?;
	// Execute the sql query in the database
	match db.import(sql, &session).await {
		Ok(res) => match accept.as_deref() {
			// Simple serialization
			Some(Accept::ApplicationJson) => Ok(output::json(&output::simplify(res))),
			Some(Accept::ApplicationCbor) => Ok(output::cbor(&output::simplify(res))),
			Some(Accept::ApplicationPack) => Ok(output::pack(&output::simplify(res))),
			// Return nothing
			Some(Accept::ApplicationOctetStream) => Ok(output::none()),
			// Internal serialization
			Some(Accept::Surrealdb) => Ok(output::full(&res)),
			// An incorrect content-type was requested
			_ => Err(Error::InvalidType),
		},
		// There was an error when executing the query
		Err(err) => Err(Error::from(err)),
	}
}
