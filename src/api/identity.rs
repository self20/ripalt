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

use super::*;

use actix_web::middleware::{Middleware, Response, Started};
use actix_web::error::{Error as AwError, Result as AwResult};
use jwt::{decode, Validation};
use std::rc::Rc;

/// Identity policy definition.
pub trait IdentityPolicy<S>: Sized + 'static {
    type Identity: Identity;
    type Future: Future<Item = Self::Identity, Error = AwError>;

    /// Parse the session from request and load data from a service identity.
    fn from_request(&self, request: &mut HttpRequest<S>) -> Self::Future;
}

pub trait Identity {
    fn identity(&self) -> Option<&str>;

    fn forget(&mut self);

    fn credentials(&self) -> Option<(&Uuid, &Uuid)>;

    fn user_id(&self) -> Option<&Uuid> {
        self.credentials().map(|s| s.0 )
    }

    fn group_id(&self) -> Option<&Uuid> {
        self.credentials().map(|s| s.1 )
    }
}

pub trait RequestIdentity {
    /// Return the claimed identity of the user associated request or
    /// ``None`` if no identity can be found associated with the request.
    fn identity(&self) -> Option<&str>;

    /// This method is used to 'forget' the current identity on subsequent
    /// requests.
    fn forget(&mut self);

    fn credentials(&self) -> Option<(&Uuid, &Uuid)>;

    fn user_id(&self) -> Option<&Uuid> {
        self.credentials().map(|s| s.0 )
    }

    fn group_id(&self) -> Option<&Uuid> {
        self.credentials().map(|s| s.1 )
    }
}

impl<S> RequestIdentity for HttpRequest<S> {
    fn identity(&self) -> Option<&str> {
        if let Some(id) = self.extensions_ro().get::<IdentityBox>() {
            return id.0.identity();
        }
        None
    }

    fn forget(&mut self) {
        if let Some(id) = self.extensions_mut().get_mut::<IdentityBox>() {
            return id.0.forget();
        }
    }

    fn credentials(&self) -> Option<(&Uuid, &Uuid)> {
        if let Some(id) = self.extensions_ro().get::<IdentityBox>() {
            return id.0.credentials();
        }
        None
    }
}

/// Request identity middleware
///
/// ```rust
/// # extern crate actix;
/// # extern crate actix_web;
/// use actix_web::App;
/// use actix_web::middleware::identity::{IdentityService, CookieIdentityPolicy};
///
/// fn main() {
///    let app = App::new().middleware(
///        IdentityService::new(                      // <- create identity middleware
///            CookieIdentityPolicy::new(&[0; 32])    // <- create cookie session backend
///               .name("auth-cookie")
///               .secure(false))
///    );
/// }
/// ```
pub struct IdentityService<T> {
    backend: T,
}

impl<T> IdentityService<T> {
    /// Create new identity service with specified backend.
    pub fn new(backend: T) -> Self {
        IdentityService { backend }
    }
}

struct IdentityBox(Box<Identity>);

#[doc(hidden)]
unsafe impl Send for IdentityBox {}
#[doc(hidden)]
unsafe impl Sync for IdentityBox {}

impl<S: 'static, T: IdentityPolicy<S>> Middleware<S> for IdentityService<T> {
    fn start(&self, req: &mut HttpRequest<S>) -> AwResult<Started> {
        let mut req = req.clone();

        let fut = self.backend
            .from_request(&mut req)
            .then(move |res| match res {
                Ok(id) => {
                    req.extensions().insert(IdentityBox(Box::new(id)));
                    FutOk(None)
                }
                Err(err) => FutErr(err),
            });
        Ok(Started::Future(Box::new(fut)))
    }

    fn response(
        &self, req: &mut HttpRequest<S>, resp: HttpResponse
    ) -> AwResult<Response> {
        if let Some(_id) = req.extensions().remove::<IdentityBox>() {
            Ok(Response::Done(resp))
        } else {
            Ok(Response::Done(resp))
        }
    }
}


pub struct ApiIdentityPolicy(Rc<ApiIdentityInner>);

impl ApiIdentityPolicy {
    pub fn new(key: &[u8]) -> ApiIdentityPolicy {
        ApiIdentityPolicy(Rc::new(ApiIdentityInner::new(key)))
    }
}

impl<S> IdentityPolicy<S> for ApiIdentityPolicy {
    type Identity = ApiIdentity;
    type Future = FutureResult<ApiIdentity, actix_web::Error>;

    fn from_request(&self, request: &mut HttpRequest<S>) -> Self::Future {
        let identity = self.0.load(request);
        if identity.is_some() {
            FutOk(ApiIdentity::new(identity))
        } else {
            FutErr(actix_web::error::ErrorUnauthorized("unauthorized"))
        }
    }
}

pub struct ApiIdentity {
    identity: Option<(Uuid, Uuid)>,
    str_identity: Option<String>
}

impl ApiIdentity {
    pub fn new(identity: Option<(Uuid, Uuid)>) -> ApiIdentity {
        let str_identity = identity.map(|s| s.0.to_string());
        ApiIdentity{identity, str_identity}
    }
}

impl Identity for ApiIdentity {
    fn identity(&self) -> Option<&str> {
        self.str_identity.as_ref().map(|s| s.as_ref() )
    }

    fn forget(&mut self) {
        self.identity = None;
    }

    fn credentials(&self) -> Option<(&Uuid, &Uuid)> {
        self.identity.as_ref().map(|s| (&s.0, &s.1) )
    }
}


struct ApiIdentityInner {
    key: Vec<u8>,
}

impl ApiIdentityInner {
    fn new(key: &[u8]) -> ApiIdentityInner {
        ApiIdentityInner { key: key.to_vec() }
    }

    fn load<S>(&self, req: &mut HttpRequest<S>) -> Option<(Uuid, Uuid)> {
        let from_session = session_creds(req);
        if from_session.is_some() {
            return from_session;
        }
        if let Some(header) = req.headers().get("authorization") {
            if let Ok(header) = header.to_str() {
                if header.to_lowercase().starts_with("bearer") {
                    let validation = Validation::default();
                    let token_data = match decode::<Claims>(&header[7..], &self.key, &validation) {
                        Ok(c) => c,
                        Err(_) => return None,
                    };
                    let Claims {iat: _, user_id, group_id } = token_data.claims;
                    return Some((user_id, group_id));
                }
            }
        }
        None
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    iat: i64,
    user_id: Uuid,
    group_id: Uuid,
}