use async_graphql::*;

use crate::{
    model::user::{Badge, Status, User, Theme},
    util::{Cx, ReferrableWithId},
};

#[Object]
impl User {
    async fn id(&self) -> ID {
        ID(self.id.id.to_string())
    }
    async fn tag_name(&self) -> &str {
        &self.tag.0
    }
    async fn tag_discriminator(&self) -> &[i32] {
        &self.tag.1
    }
    async fn tag(&self) -> String {
        self.tag_fmt()
    }
    async fn display_name(&self) -> &str {
        &self.display_name
    }

    async fn friends(&self, context: &Context<'_>) -> FieldResult<Vec<User>> {
        if context.cx().ref_user()?.id() != <Self as ReferrableWithId>::id(self) {
            return Ok(vec![]);
        }
        Ok(self.get_friends(context.cx().surreal()).await?)
    }

    async fn badges(&self) -> &[Badge] {
        &self.badges
    }
    async fn status(&self) -> Status {
        self.status
    }

    async fn theme(&self) -> Theme {
        self.theme
    }
}
