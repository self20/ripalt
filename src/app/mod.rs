/*
 * ripalt
 * Copyright (C) 2018 Daniel Müller
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License
 * along with this program.  If not, see <http://www.gnu.org/licenses/>.
 */

//! Application Handlers

use super::*;

use actix_web::{HttpMessage, HttpRequest, HttpResponse, http::{header, NormalizePath}, Either, FutureResponse};
use actix_web::AsyncResponder;
use actix_web::middleware::{csrf, CookieSessionBackend, DefaultHeaders, ErrorHandlers, Logger,
                            SessionStorage};
use uuid::Uuid;

use std::collections::HashMap;
use std::fmt::Write;

use models::User;
use template::TemplateContainer;
use tera::Context;

pub mod index;
pub mod signup;
pub mod login;
pub mod torrent;

type SyncResponse<T> = actix_web::Result<T>;

fn sync_redirect(loc: &str) -> SyncResponse<HttpResponse> {
    Ok(redirect(loc))
}
//fn async_redirect(loc: &str) -> FutureResponse<HttpResponse> {
//    Box::new(future::ok(redirect(loc)))
//}
fn redirect(loc: &str) -> HttpResponse {
    HttpResponse::SeeOther().header(header::LOCATION, loc.to_owned()).finish()
}

pub fn build(
    db: Addr<Syn, DbExecutor>,
    tpl: TemplateContainer,
    acl: Arc<RwLock<Acl>>,
) -> App<State> {
    //    let redis = env::var("REDIS").unwrap_or(String::from("127.0.0.1::6379"));
    let session_secret = util::from_hex(&SETTINGS.read().unwrap().session_secret).unwrap();
    let session_name = &SETTINGS.read().unwrap().session_name[..];
    let session_secure = &SETTINGS.read().unwrap().https;

    let mut state = State::new(db, acl);
    state.set_template(tpl);
    App::with_state(state)
        .middleware(Logger::default())
        .middleware(DefaultHeaders::new().header("X-Version", env!("CARGO_PKG_VERSION")))
        .middleware(csrf::CsrfFilter::new().allow_xhr().allowed_origin("http://localhost:8081"))
//        .middleware(SessionStorage::new(RedisSessionBackend::new(
//            redis,
//            &session_secret,
//        ).cookie_name(session_name)))
        .middleware(SessionStorage::new(
            CookieSessionBackend::signed(&session_secret)
                .name(session_name).secure(*session_secure)))
        .middleware(
            ErrorHandlers::new().handler(StatusCode::INTERNAL_SERVER_ERROR, app::server_error),
        )
        .handler("/static", StaticFiles::new("webroot/static/"))
        .resource("/", |r| {
            r.name("index");
            r.route().filter(require_user()).f(app::index::authenticated);

            r.f(app::index::index);
        })
        .resource("/signup", |r| {
            r.name("signup#signup");
            r.method(Method::GET).f(app::signup::signup);
            r.name("signup#take_signup");
            r.method(Method::POST).a(app::signup::take_signup);
        })
        .resource("/confirm/{id}", |r| {
            r.name("signup#confirm");
            r.method(Method::GET).a(app::signup::confirm)
        })
        .resource("/login", |r| {
            r.name("login#login");
            r.method(Method::GET).f(app::login::login);
            r.name("login#take_login");
            r.method(Method::POST).a(app::login::take_login);
        })
        .resource("/logout", |r| {
            r.name("login#logout");
            r.method(Method::GET).f(app::login::logout)
        })
        .resource("/torrents", |r| {
            r.name("torrent#list");
            r.route().filter(require_user()).a(app::torrent::list);
        })
        .resource("/torrent/upload", |r| {
            r.name("torrent#list");
            r.method(Method::GET).filter(require_user()).f(app::torrent::new);
            r.name("torrent#upload");
            r.method(Method::POST).filter(require_user()).f(app::torrent::create);
        })
        .resource("/torrent/download/{id}", |r| {
            r.name("torrent#download");
            r.method(Method::GET).filter(require_user()).f(app::torrent::download);
        })
        .resource("/torrent/{id}", |r| {
            r.name("torrent#read");
            r.method(Method::GET).filter(require_user()).f(app::torrent::torrent);
        })
        .default_resource(|r| r.f(app::not_found))
}

pub fn not_found(req: HttpRequest<State>) -> SyncResponse<HttpResponse> {
    use actix_web::dev::Handler;

    let mut h = NormalizePath::default();
    let resp = h.handle(req.clone());

    if resp.status().is_server_error() || resp.status().is_client_error() {
        Ok(render_error(&req, resp))
    } else {
        Ok(resp)
    }
}

pub fn server_error(
    req: &mut HttpRequest<State>,
    resp: HttpResponse,
) -> SyncResponse<actix_web::middleware::Response> {
    Ok(actix_web::middleware::Response::Done(render_error(
        req,
        resp,
    )))
}

fn render_error(req: &HttpRequest<State>, resp: HttpResponse) -> HttpResponse {
    let mut context = HashMap::new();
    context.insert("status", format!("{}", resp.status()));
    context.insert("uri", format!("{}", req.uri()));
    let mut headers = String::new();
    for (n, v) in req.headers() {
        writeln!(&mut headers, "{:?}: {:?}", n, v).unwrap();
    }
    context.insert("headers", headers);
    context.insert("error", "Internal Server Error".to_string());

    let tpl = if resp.status().is_server_error() {
        "error/5xx.html"
    } else {
        "error/4xx.html"
    };

    let mut new_resp: HttpResponse = match Template::render(&req.state().template(), tpl, &context)
    {
        Ok(r) => r.into(),
        Err(e) => {
            return resp.into_builder()
                .header(header::CONTENT_TYPE, "text/plain")
                .status(StatusCode::from_u16(500u16).unwrap())
                .body(format!("Internal Server Error\n{}", e))
        }
    };
    {
        let status = new_resp.status_mut();
        *status = resp.status();
    }

    new_resp
}