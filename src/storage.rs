use std::{collections::HashMap, io::Read, default::default};

use crate::{model::user::User, util::Ref};

pub struct Storage {
    avatars: HashMap<avatar::AvRef, avatar::Av>,
}

mod avatar {
    use derive_more::Display;

    #[derive(Display, Debug, Clone, PartialEq, Eq)]
    #[display(fmt = "storage/avatar/{r}.{ft}")]
    pub struct Av {
        pub r: AvRef,
        pub ft: AvFt,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Hash, Display)]
    #[display(fmt = "{k}/{i}")]
    pub struct AvRef {
        pub k: AvK,
        pub i: String,
    }

    #[derive(PartialEq, Eq, Clone, Debug, Display)]
    pub enum AvFt {
        #[display(fmt = "gif")]
        Anim,
        #[display(fmt = "png")]
        Static,
    }

    #[derive(PartialEq, Eq, Clone, Debug, Display, Hash)]
    pub enum AvK {
        #[display(fmt = "user")]
        U,
        #[display(fmt = "guild")]
        G,
    }
}

use async_graphql::UploadValue;
use async_std::{
    fs::{create_dir_all, File},
    path::{Path, PathBuf},
};
pub use avatar::AvFt as AvatarFiletype;
pub use avatar::AvK as AvatarKind;
use futures_util::AsyncWriteExt;

async fn just_create_or_something(path: impl AsRef<Path>) -> async_std::io::Result<()> {
    if let Err(e) = create_dir_all(path).await {
        match e.kind() {
            std::io::ErrorKind::AlreadyExists => {}
            _ => return Err(e),
        }
    }
    Ok(())
}

impl Storage {
    pub fn new() -> Self {
        Self { avatars: default() }
    }

    pub async fn init_fs(&self) -> async_std::io::Result<()> {
        just_create_or_something("./storage/avatar/user").await?;
        just_create_or_something("./storage/avatar/guild").await?;
        Ok(())
    }

    pub fn tide(&self, tide: &mut tide::Server<crate::http::HttpState>) -> std::io::Result<()> {
        let mut storage = tide.at("/storage");
        storage
            .at("/avatar/user")
            .serve_dir("storage/avatar/user")?;
        Ok(())
    }

    pub fn get_user_avatar(&self, id: String, kind: AvatarKind) -> Option<String> {
        let r = avatar::AvRef {
            k: kind,
            i: id,
        };
        self.avatars.get(&r).map(ToString::to_string)
    }

    pub async fn put_avatar(
        &mut self,
        id: String,
        kind: AvatarKind,
        avatar: Vec<u8>,
        ft: AvatarFiletype,
    ) -> async_std::io::Result<()> {
        let r = avatar::AvRef { k: kind, i: id };
        let a = avatar::Av {
            ft,
            r: r.clone(),
        };

        let path = PathBuf::from(a.to_string());
        let mut file = File::create(&path).await?;
        file.write_all(&avatar).await?;

        self.avatars.insert(r, a);

        Ok(())
    }

    pub async fn put_avatar_graphql(
        &mut self,
        id: String,
        kind: AvatarKind,
        ft: AvatarFiletype,
        upload: UploadValue,
    ) -> async_std::io::Result<()> {
        let mut reader = upload.into_read();
        let mut avatar = vec![];
        reader.read(&mut avatar)?;
        self.put_avatar(id, kind, avatar, ft).await
    }
}
