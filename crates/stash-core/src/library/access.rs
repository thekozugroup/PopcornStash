//! Who sees what (section 7, Library access). The one rule lives here twice, as a SQL
//! predicate for queries ([`push_visible`]) and as [`Access::sees`] for single items and
//! live events, and a test keeps the two in agreement.

use std::collections::{HashMap, HashSet};

use sqlx::{QueryBuilder, Sqlite, SqliteConnection};

use crate::Result;
use crate::db::Db;
use crate::models::{Category, ItemFacts, ItemRow, ItemStatus, Place, Role};

/// The caller as far as access goes: their role and whether the credential in use has
/// the `full` scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Viewer {
    pub user_id: String,
    pub role: Role,
    pub full: bool,
}

impl Viewer {
    /// Role owner with the `full` scope: sees every item, library and download.
    pub fn full_owner(&self) -> bool {
        self.role == Role::Owner && self.full
    }

    /// Role owner with any credential: sees every library.
    pub fn sees_every_library(&self) -> bool {
        self.role == Role::Owner
    }
}

/// A viewer with the library rows that decide what they see, loaded once (per request
/// or per event stream) so single items and events can be checked without queries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Access {
    pub viewer: Viewer,
    /// Ids of the libraries the viewer sees.
    pub libraries: HashSet<String>,
    /// Kind to the library that claims it.
    pub claims: HashMap<Category, String>,
}

impl Access {
    pub async fn load(db: &Db, viewer: &Viewer) -> Result<Self> {
        let mut conn = db.acquire().await?;
        Self::load_conn(&mut conn, viewer).await
    }

    pub async fn load_conn(conn: &mut SqliteConnection, viewer: &Viewer) -> Result<Self> {
        let libraries: Vec<String> = if viewer.sees_every_library() {
            sqlx::query_scalar("SELECT id FROM libraries")
                .fetch_all(&mut *conn)
                .await?
        } else {
            sqlx::query_scalar(
                "SELECT id FROM libraries WHERE visibility = 'everyone'
                 UNION SELECT library_id FROM library_members WHERE user_id = ?",
            )
            .bind(&viewer.user_id)
            .fetch_all(&mut *conn)
            .await?
        };
        let claims: Vec<(Category, String)> =
            sqlx::query_as("SELECT category, library_id FROM library_claims")
                .fetch_all(&mut *conn)
                .await?;
        Ok(Self {
            viewer: viewer.clone(),
            libraries: libraries.into_iter().collect(),
            claims: claims.into_iter().collect(),
        })
    }

    pub fn full_owner(&self) -> bool {
        self.viewer.full_owner()
    }

    pub fn sees_library(&self, id: &str) -> bool {
        self.libraries.contains(id)
    }

    /// The library that claims `category`, when this viewer sees it.
    pub fn visible_claimer(&self, category: Category) -> Option<&str> {
        self.claims
            .get(&category)
            .map(String::as_str)
            .filter(|id| self.sees_library(id))
    }

    /// The rule of section 7 for one item.
    pub fn sees(&self, item: &ItemFacts) -> bool {
        if self.full_owner() {
            return true;
        }
        let mine = item.added_by.as_deref() == Some(self.viewer.user_id.as_str());
        match item.place {
            Place::Library => item
                .library_id
                .as_deref()
                .is_some_and(|id| self.sees_library(id)),
            Place::OtherFiles => mine,
            Place::Incoming => {
                mine || (item.status == ItemStatus::NeedsReview
                    && self.visible_claimer(item.category).is_some())
            }
            Place::Folder | Place::Trash => false,
        }
    }

    pub fn sees_row(&self, row: &ItemRow) -> bool {
        self.sees(&row.facts())
    }
}

/// `AND (<the viewer's predicate on items aliased i>)`.
pub fn push_visible(qb: &mut QueryBuilder<'_, Sqlite>, viewer: &Viewer) {
    if viewer.full_owner() {
        return;
    }
    let uid = viewer.user_id.clone();
    qb.push(" AND ((i.place = 'library' AND ");
    push_library_visible(qb, viewer, "i.library_id");
    qb.push(") OR (i.place = 'other_files' AND i.added_by = ")
        .push_bind(uid.clone())
        .push(") OR (i.place = 'incoming' AND (i.added_by = ")
        .push_bind(uid)
        .push(
            " OR (i.status = 'needs_review' AND EXISTS (SELECT 1 FROM library_claims vc \
             WHERE vc.category = i.category AND ",
        );
    push_library_visible(qb, viewer, "vc.library_id");
    qb.push(")))))");
}

/// `<column> is a library the viewer sees`.
pub fn push_library_visible(qb: &mut QueryBuilder<'_, Sqlite>, viewer: &Viewer, column: &str) {
    if viewer.sees_every_library() {
        qb.push(format!("{column} IS NOT NULL"));
    } else {
        qb.push(format!(
            "{column} IN (SELECT id FROM libraries WHERE visibility = 'everyone' \
             UNION SELECT library_id FROM library_members WHERE user_id = "
        ))
        .push_bind(viewer.user_id.clone())
        .push(")");
    }
}

/// The item when it exists and the viewer may see it (trashed items included: routes
/// that never show the trash refuse those themselves).
pub async fn visible_row(db: &Db, viewer: &Viewer, id: &str) -> Result<Option<ItemRow>> {
    let mut qb = QueryBuilder::<Sqlite>::new("SELECT i.* FROM items i WHERE i.id = ");
    qb.push_bind(id.to_string());
    push_visible(&mut qb, viewer);
    Ok(qb.build_query_as::<ItemRow>().fetch_optional(db).await?)
}

/// An item id with what decides who sees it.
type FactsRow = (
    String,
    Option<String>,
    Place,
    Option<String>,
    ItemStatus,
    Category,
);

/// What decides who sees each of these items, for those that still exist.
pub async fn facts_of(db: &Db, ids: &[String]) -> Result<HashMap<String, ItemFacts>> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let json = serde_json::to_string(ids)?;
    let rows: Vec<FactsRow> = sqlx::query_as(
        "SELECT id, library_id, place, added_by, status, category FROM items
             WHERE id IN (SELECT value FROM json_each(?))",
    )
    .bind(json)
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, library_id, place, added_by, status, category)| {
            (
                id,
                ItemFacts {
                    library_id,
                    place,
                    added_by,
                    status,
                    category,
                },
            )
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::repo::tests::{add_user, row};

    async fn add_member(db: &Db, id: &str) {
        sqlx::query("INSERT INTO users (id, display_name, role, webauthn_id, created_at) VALUES (?, ?, 'member', ?, 0)")
            .bind(id)
            .bind(id)
            .bind(format!("w-{id}"))
            .execute(db)
            .await
            .unwrap();
    }

    /// The SQL predicate and `Access::sees` agree for every viewer and every kind of item.
    #[tokio::test]
    async fn sql_and_memory_agree() {
        let db = crate::db::open_memory().await.unwrap();
        add_user(&db, "owner").await;
        add_member(&db, "ann").await;
        add_member(&db, "bob").await;
        // A private library only Ann sees, holding photos.
        sqlx::query(
            "INSERT INTO libraries (id, name, icon, content_type, structure, folder, visibility, position, created_at, updated_at)
             VALUES ('kids', 'Kids', 'baby', 'video', 'one_offs', 'Kids', 'selected', 4, 0, 0)",
        )
        .execute(&db)
        .await
        .unwrap();
        sqlx::query("INSERT INTO library_members (library_id, user_id) VALUES ('kids', 'ann')")
            .execute(&db)
            .await
            .unwrap();
        sqlx::query("UPDATE library_claims SET library_id = 'kids' WHERE category = 'home_video'")
            .execute(&db)
            .await
            .unwrap();
        let mut n = 0;
        let mut add = |rel: &str, status: ItemStatus, category: Category, by: Option<&str>| {
            n += 1;
            let mut r = row(&format!("i{n:02}"), &format!("Item {n}"));
            r.rel_path = rel.to_string();
            r.status = status;
            r.category = category;
            r.added_by = by.map(str::to_string);
            r
        };
        let rows = vec![
            add("Movies/A/A.mkv", ItemStatus::Ready, Category::Movie, None),
            add("Kids/b.mkv", ItemStatus::Ready, Category::HomeVideo, None),
            add("Kids/c.mkv", ItemStatus::Missing, Category::HomeVideo, None),
            add(
                "Other/d.pdf",
                ItemStatus::Ready,
                Category::Other,
                Some("ann"),
            ),
            add(
                "Other/e.pdf",
                ItemStatus::Ready,
                Category::Other,
                Some("bob"),
            ),
            add("Other/f.pdf", ItemStatus::Ready, Category::Other, None),
            add(
                "Inbox/g.mkv",
                ItemStatus::NeedsReview,
                Category::HomeVideo,
                None,
            ),
            add(
                "Inbox/h.mkv",
                ItemStatus::NeedsReview,
                Category::Movie,
                Some("bob"),
            ),
            add("Inbox/i.mkv", ItemStatus::Ready, Category::Movie, None),
            add(
                "Inbox/j.pdf",
                ItemStatus::NeedsReview,
                Category::Other,
                Some("ann"),
            ),
            add(
                "Family/k.mkv",
                ItemStatus::Ready,
                Category::HomeVideo,
                Some("ann"),
            ),
            add("loose.mkv", ItemStatus::Ready, Category::Movie, None),
            add(
                ".popcornstash/Trash/x/l.mkv",
                ItemStatus::Trashed,
                Category::Movie,
                Some("ann"),
            ),
            add(
                "Downloads/Completed/m/m.mkv",
                ItemStatus::NeedsReview,
                Category::Episode,
                None,
            ),
        ];
        for r in &rows {
            assert!(crate::library::repo::insert_item(&db, r).await.unwrap());
        }
        let viewers = [
            Viewer {
                user_id: "owner".into(),
                role: Role::Owner,
                full: true,
            },
            Viewer {
                user_id: "owner".into(),
                role: Role::Owner,
                full: false,
            },
            Viewer {
                user_id: "ann".into(),
                role: Role::Member,
                full: true,
            },
            Viewer {
                user_id: "bob".into(),
                role: Role::Member,
                full: true,
            },
            Viewer {
                user_id: "bob".into(),
                role: Role::Member,
                full: false,
            },
        ];
        for viewer in &viewers {
            let access = Access::load(&db, viewer).await.unwrap();
            for r in &rows {
                let stored = crate::library::repo::get(&db, &r.id)
                    .await
                    .unwrap()
                    .unwrap();
                let sql = visible_row(&db, viewer, &r.id).await.unwrap().is_some();
                assert_eq!(
                    sql,
                    access.sees_row(&stored),
                    "{viewer:?} {} ({:?})",
                    r.rel_path,
                    stored.place
                );
            }
        }
        // Spot checks of the rule itself.
        let ann = Access::load(&db, &viewers[2]).await.unwrap();
        let bob = Access::load(&db, &viewers[3]).await.unwrap();
        let tv = Access::load(&db, &viewers[1]).await.unwrap();
        let mut stored = HashMap::new();
        for r in &rows {
            let row = crate::library::repo::get(&db, &r.id)
                .await
                .unwrap()
                .unwrap();
            stored.insert(r.id.clone(), row);
        }
        let sees = |a: &Access, id: &str| a.sees_row(&stored[id]);
        assert!(sees(&ann, "i02") && !sees(&bob, "i02"), "a private library");
        assert!(
            sees(&tv, "i02"),
            "an owner's limited token sees every library"
        );
        assert!(
            !sees(&tv, "i11") && !sees(&tv, "i06"),
            "but not folders or others' files"
        );
        assert!(
            sees(&ann, "i04") && !sees(&bob, "i04"),
            "Other files are private"
        );
        assert!(
            sees(&ann, "i07") && !sees(&bob, "i07"),
            "waiting, claimed by Kids"
        );
        assert!(
            sees(&bob, "i08") && sees(&ann, "i08"),
            "Bob added it; Movies claims it"
        );
        assert!(!sees(&ann, "i09"), "ignored files are their adder's only");
        assert!(
            !sees(&ann, "i11") && !sees(&ann, "i13"),
            "folders and the trash"
        );
    }
}
