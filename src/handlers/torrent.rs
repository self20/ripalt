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

#[derive(Debug)]
pub struct NewTorrentMsg {
    pub name: String,
    pub description: String,
    pub meta_file: Vec<u8>,
    pub nfo_file: Vec<u8>,
    //    pub image_file: Option<Vec<u8>>,
    pub category: Uuid,
    pub user: Uuid,
    pub info_hash: Vec<u8>,
    pub size: i64,
    pub files: Vec<NewFile>,
}

impl NewTorrentMsg {
    fn insert_meta(&self, id: &Uuid, conn: &DbConn) -> Result<usize> {
        let meta = models::torrent::NewTorrentMetaFile{
            id,
            data: &self.meta_file,
        };

        meta.create(&conn)
    }

    fn insert_files(&self, id: &Uuid, conn: &DbConn) -> Result<()> {
        for f in &self.files {
            let file_id = Uuid::new_v4();
            let file = models::torrent::NewTorrentFile{
                id: &file_id,
                torrent_id: id,
                file_name: &f.file_name[..],
                size: f.size
            };

            file.create(&conn)?;
        }

        Ok(())
    }

    fn insert_nfo(&self, id: &Uuid, conn: &DbConn) -> Result<usize> {
        let nfo_id = Uuid::new_v4();
        let nfo = models::torrent::NewTorrentNFO{
            id: &nfo_id,
            torrent_id: id,
            data: &self.nfo_file,
        };

        nfo.create(&conn)
    }
}

impl Message for NewTorrentMsg {
    type Result = Result<models::Torrent>;
}

impl Handler<NewTorrentMsg> for DbExecutor {
    type Result = Result<models::Torrent>;

    fn handle(
        &mut self,
        msg: NewTorrentMsg,
        _: &mut Self::Context,
    ) -> <Self as Handler<NewTorrentMsg>>::Result {
        let conn = self.conn();

        let _category = match models::category::Category::find(&msg.category, &conn) {
            Some(c) => c,
            None => bail!("category not found"),
        };

        let torrent = models::Torrent::create(
            &msg.name,
            &msg.description,
            &msg.info_hash,
            &msg.category,
            &msg.user,
            msg.size,
            &conn,
        )?;

        msg.insert_meta(&torrent.id, &conn)?;
        msg.insert_files(&torrent.id, &conn)?;
        msg.insert_nfo(&torrent.id, &conn)?;

        Ok(torrent)
    }
}

#[derive(Debug)]
pub struct UpdateTorrentMsg {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub nfo_file: Option<Vec<u8>>,
    pub category: Uuid,
    pub user: UserSubjectMsg,
}

impl UpdateTorrentMsg {
    pub fn new(id: Uuid, user: UserSubjectMsg) -> Self {
        UpdateTorrentMsg{
            id,
            name: Default::default(),
            description: Default::default(),
            nfo_file: Default::default(),
            category: Default::default(),
            user,
        }
    }

    fn replace_nfo(&self, conn: &DbConn) -> Result<usize> {
        let nfo_file = match self.nfo_file {
            Some(ref nfo_file) => nfo_file,
            None => bail!("no new nfo uploaded"),
        };

        use schema::torrent_nfos::dsl as n;
        let db: &PgConnection = conn;
        let res = diesel::delete(schema::torrent_nfos::table)
            .filter(n::torrent_id.eq(&self.id))
            .execute(db);

        match res {
            Ok(_) => {
                let nfo_id = Uuid::new_v4();
                let nfo = models::torrent::NewTorrentNFO{
                    id: &nfo_id,
                    torrent_id: &self.id,
                    data: nfo_file,
                };

                nfo.create(&conn)
            }
            Err(e) => Err(format!("failed to create new nfo: {}", e).into()),
        }
    }
}

impl Message for UpdateTorrentMsg {
    type Result = Result<usize>;
}

impl Handler<UpdateTorrentMsg> for DbExecutor {
    type Result = Result<usize>;

    fn handle(
        &mut self,
        msg: UpdateTorrentMsg,
        _: &mut Self::Context,
    ) -> <Self as Handler<UpdateTorrentMsg>>::Result {
        let conn = self.conn();
        let torrent = models::Torrent::find(&msg.id, &conn).ok_or_else(|| "torrent not found")?;
        let subj = UserSubject::from(&msg.user);
        if !subj.may_write(&torrent) {
            bail!("user is not allowed");
        }

        let _category = match models::category::Category::find(&msg.category, &conn) {
            Some(c) => c,
            None => bail!("category not found"),
        };

        let torrent = models::torrent::UpdateTorrent{
            name: &msg.name,
            category_id: &msg.category,
            description: &msg.description,
        };

        if msg.nfo_file.is_some() {
            msg.replace_nfo(&conn)?;
        }

        torrent.update(&msg.id, &conn)
    }
}

#[derive(Debug, Default)]
pub struct NewTorrentBuilder {
    name: String,
    description: String,
    meta_file: Vec<u8>,
    nfo_file: Vec<u8>,
    category: Uuid,
    user: Uuid,
}

impl NewTorrentBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn name<T>(&mut self, name: &T) -> &Self
    where
        T: ToString,
    {
        self.name = name.to_string();
        self
    }

    pub fn description<T>(&mut self, desc: &T) -> &Self
    where
        T: ToString,
    {
        self.description = desc.to_string();
        self
    }

    pub fn category(&mut self, category: Uuid) -> &Self {
        self.category = category;
        self
    }

    #[allow(dead_code)]
    pub fn user(&mut self, user: Uuid) -> &Self {
        self.user = user;
        self
    }

    pub fn raw_meta(&mut self, meta_file: Vec<u8>) -> &Self {
        assert!(!meta_file.is_empty(), "meta file is empty");
        self.meta_file = meta_file;
        self
    }

    pub fn nfo(&mut self, nfo_file: Vec<u8>) -> &Self {
        assert!(!nfo_file.is_empty(), "nfo_file is empty");
        self.nfo_file = nfo_file;
        self
    }

    pub fn finish(self) -> Result<NewTorrentMsg> {
        let info_hash = util::torrent::info_hash(&self.meta_file)?;
        let files: Vec<NewFile> = util::torrent::files(&self.meta_file)?
            .into_iter()
            .map(|(file_name, size)| NewFile { file_name, size })
            .collect();
        let size = files.iter().fold(0i64, |acc, ref x| acc + x.size);

        Ok(NewTorrentMsg {
            name: self.name,
            description: self.description,
            category: self.category,
            user: self.user,
            meta_file: self.meta_file,
            nfo_file: self.nfo_file,
            size,
            info_hash,
            files,
        })
    }
}

pub struct LoadCategoriesMsg {}

impl Message for LoadCategoriesMsg {
    type Result = Result<Vec<models::Category>>;
}

impl Handler<LoadCategoriesMsg> for DbExecutor {
    type Result = Result<Vec<models::Category>>;

    fn handle(
        &mut self,
        _msg: LoadCategoriesMsg,
        _: &mut Self::Context,
    ) -> <Self as Handler<LoadCategoriesMsg>>::Result {
        use schema::categories::dsl;
        let conn = self.conn();
        let db: &PgConnection = &conn;

        dsl::categories
            .order(dsl::name.asc())
            .load::<models::Category>(db)
            .chain_err(|| "failed to load categories")
    }
}

#[derive(Debug)]
pub struct NewFile {
    file_name: String,
    size: i64,
}

pub struct LoadTorrentMsg {
    id: Uuid,
    user_id: Uuid,
}

impl LoadTorrentMsg {
    pub fn new(id: &Uuid, user_id: &Uuid) -> LoadTorrentMsg {
        LoadTorrentMsg {
            id: *id,
            user_id: *user_id,
        }
    }
}

impl Message for LoadTorrentMsg {
    type Result = Result<models::torrent::TorrentMsg>;
}

impl Handler<LoadTorrentMsg> for DbExecutor {
    type Result = Result<models::torrent::TorrentMsg>;

    fn handle(&mut self, msg: LoadTorrentMsg, _: &mut Self::Context) -> <Self as Handler<LoadTorrentMsg>>::Result {
        let conn = self.conn();
        let torrent = models::torrent::TorrentMsg::find(&msg.id, &conn);
        torrent.map(|mut t| {
            t.timezone = util::user::user_timezone(&msg.user_id, &conn);
            t
        })
    }
}


pub struct LoadTorrentMetaMsg {
    pub id: Uuid,
    pub uid: Uuid,
}

impl Message for LoadTorrentMetaMsg {
    type Result = Result<(String, Vec<u8>, Vec<u8>)>;
}

impl Handler<LoadTorrentMetaMsg> for DbExecutor {
    type Result = Result<(String, Vec<u8>, Vec<u8>)>;

    fn handle(&mut self, msg: LoadTorrentMetaMsg, _: &mut Self::Context) -> <Self as Handler<LoadTorrentMetaMsg>>::Result {
        let conn = self.conn();
        let torrent = models::torrent::Torrent::find(&msg.id, &conn).ok_or("torrent not found")?;
        let meta_file = models::torrent::TorrentMetaFile::find(&msg.id, &conn).ok_or("meta file not found")?;
        let passcode = models::User::find(&msg.uid, &conn).ok_or("user not found")?.passcode;
        let name = format!("{}.torrent", torrent.name);

        Ok((name, meta_file.data, passcode))
    }
}

pub struct LoadTorrentNfoMsg {
    pub id: Uuid,
}

impl Message for LoadTorrentNfoMsg {
    type Result = Result<(String, Vec<u8>)>;
}

impl Handler<LoadTorrentNfoMsg> for DbExecutor {
    type Result = Result<(String, Vec<u8>)>;

    fn handle(&mut self, msg: LoadTorrentNfoMsg, _: &mut Self::Context) -> <Self as Handler<LoadTorrentNfoMsg>>::Result {
        let conn = self.conn();
        let torrent = models::torrent::Torrent::find(&msg.id, &conn).ok_or("torrent not found")?;
        let nfo_file = models::torrent::TorrentNFO::find_for_torrent(&msg.id, &conn).ok_or("nfo file not found")?;
        let name = format!("{}.nfo", torrent.name);

        Ok((name, nfo_file.data))
    }
}



#[derive(Debug)]
pub enum Visible {
    Visible,
    Invisible,
    All,
}

impl Default for Visible {
    fn default() -> Self {
        Visible::Visible
    }
}

impl ToString for Visible {
    fn to_string(&self) -> String {
        match self {
            Visible::Visible => String::from("visible"),
            Visible::Invisible => String::from("dead"),
            Visible::All => String::from("all"),
        }
    }
}

#[derive(Default, Debug)]
pub struct LoadTorrentListMsg {
    pub name: Option<String>,
    pub category: Option<Uuid>,
    pub user_id: Option<Uuid>,
    pub visible: Visible,
    pub page: i64,
    pub per_page: i64,
    pub current_user_id: Uuid,
}

impl LoadTorrentListMsg {
    pub fn new(user_id: &Uuid) -> Self {
        LoadTorrentListMsg {
            page: 1,
            per_page: 25,
            current_user_id: *user_id,
            ..Default::default()
        }
    }

    pub fn name(&mut self, name: String) -> &Self {
        self.name = Some(name);
        self
    }

    pub fn category(&mut self, category_id: Uuid) -> &Self {
        self.category = Some(category_id);
        self
    }

    #[allow(dead_code)]
    pub fn user(&mut self, user_id: Uuid) -> &Self {
        self.user_id = Some(user_id);
        self
    }

    pub fn visible(&mut self, visible: Visible) -> &Self {
        self.visible = visible;
        self
    }

    pub fn page(&mut self, page: i64, per_page: i64) -> &Self {
        self.page = page;
        self.per_page = per_page;
        self
    }

    pub fn query(&mut self, db: &PgConnection) -> (Vec<models::TorrentList>, i64) {
        use schema::torrent_list::dsl;
        let mut query = dsl::torrent_list.into_boxed();
        let mut query2 = dsl::torrent_list.into_boxed();

        if let Some(name) = &self.name {
            query = query.filter(dsl::name.ilike(format!("%{}%", name)));
            query2 = query2.filter(dsl::name.ilike(format!("%{}%", name)));
        }
        if let Some(category) = &self.category {
            query = query.filter(dsl::category_id.eq(category));
            query2 = query2.filter(dsl::category_id.eq(category));
        }
        if let Some(user_id) = &self.user_id {
            query = query.filter(dsl::user_id.eq(user_id));
            query2 = query2.filter(dsl::user_id.eq(user_id));
        }
        match self.visible {
            Visible::Visible => {
                query = query.filter(dsl::visible.eq(true));
                query2 = query2.filter(dsl::visible.eq(true));
            }
            Visible::Invisible => {
                query = query.filter(dsl::visible.eq(false));
                query2 = query2.filter(dsl::visible.eq(false));
            }
            Visible::All => {}
        }

        // overwrite "per page" with user defined number, if set
        self.per_page = self.user_per_page(db);

        let count = query2.count().get_result(db).unwrap();
        let list = query
            .limit(self.per_page)
            .offset((self.page - 1) * self.per_page)
            .load::<models::TorrentList>(db);

        (list.unwrap_or_default(), count)
    }

    fn user_per_page(&self, db: &PgConnection) -> i64 {
        if let Some(prop) = models::user::Property::find(&self.current_user_id, "torrents_per_page", db) {
            if let Some(number) = prop.value.as_i64() {
                return number;
            }
        }

        self.per_page
    }
}

#[derive(Debug, Default)]
pub struct TorrentListMsg {
    pub torrents: Vec<models::TorrentList>,
    pub count: i64,
    pub request: LoadTorrentListMsg,
    pub timezone: i32,
}

impl Message for LoadTorrentListMsg {
    type Result = Result<TorrentListMsg>;
}

impl Handler<LoadTorrentListMsg> for DbExecutor {
    type Result = Result<TorrentListMsg>;

    fn handle(&mut self, mut msg: LoadTorrentListMsg, _: &mut Self::Context) -> <Self as Handler<LoadTorrentListMsg>>::Result {
        let db = self.conn();
        let (list, count) = msg.query(&db);
        let timezone = util::user::user_timezone(&msg.current_user_id, &db);
        Ok(TorrentListMsg{
            torrents: list,
            count,
            request: msg,
            timezone,
        })
    }
}

pub struct DeleteTorrentMsg {
    pub id: Uuid,
    pub reason: String,
    pub user: UserSubjectMsg,
}

impl Message for DeleteTorrentMsg {
    type Result = Result<usize>;
}

impl Handler<DeleteTorrentMsg> for DbExecutor {
    type Result = Result<usize>;

    fn handle(&mut self, msg: DeleteTorrentMsg, _: &mut Self::Context) -> <Self as Handler<DeleteTorrentMsg>>::Result {
        let conn = self.conn();

        let torrent = models::Torrent::find(&msg.id, &conn).ok_or_else(|| "torrent not found")?;
        let subj = UserSubject::from(&msg.user);
        if !subj.may_delete(&torrent) {
            bail!("user is not allowed");
        }

        torrent.delete(&conn)
    }
}
