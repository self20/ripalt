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
use handlers::user::LoadUserStatsMsg;
use models::user::UserStatsMsg;

pub fn stats(req: HttpRequest<State>) -> FutureResponse<HttpResponse> {
    if let Some(user_id) = req.user_id() {
        req.state().db().send(LoadUserStatsMsg(user_id.to_owned()))
            .from_err()
            .and_then(|result: Result<UserStatsMsg>| {
                match result {
                    Ok(stats) => {
                        Ok(HttpResponse::Ok().json(stats))
                    },
                    Err(_) => Ok(HttpResponse::InternalServerError().into()),
                }
            })
            .responder()
    } else {
        Box::new(FutErr(ErrorUnauthorized("unauthorized")))
    }
}