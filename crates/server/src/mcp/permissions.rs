use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    auth::{AuthUser, McpAuthority},
    error::ApiError,
    state::AppState,
};

#[derive(Clone, Default, Deserialize, Serialize, ToSchema)]
pub struct Permissions {
    #[serde(default)]
    pub scopes: Vec<String>,
    #[serde(default)]
    pub allowed_tools: Vec<String>,
}

impl Permissions {
    pub fn allows(&self, name: &str, scope: &str) -> bool {
        self.scopes.iter().any(|s| s == scope) || self.allowed_tools.iter().any(|t| t == name)
    }

    pub fn validate(&self) -> Result<(), ApiError> {
        if self.scopes.len() > rudder_core::MCP_SCOPES.len()
            || self.scopes.iter().any(|s| !rudder_core::scope_known(s))
        {
            return Err(bad("Choose known MCP read/write scopes."));
        }
        let tools = super::tools();
        if self.allowed_tools.len() > tools.len()
            || self
                .allowed_tools
                .iter()
                .any(|name| !tools.iter().any(|t| &t.name == name))
        {
            return Err(bad(
                "Choose known MCP tool names from the permission catalog.",
            ));
        }
        Ok(())
    }

    pub fn subset_of(&self, ceiling: &Self) -> bool {
        self.scopes.iter().all(|s| ceiling.scopes.contains(s))
            && super::tools()
                .iter()
                .all(|t| !self.allows(&t.name, &t.scope) || ceiling.allows(&t.name, &t.scope))
    }
}

pub fn for_user(user: &AuthUser) -> Permissions {
    Permissions {
        scopes: user.scopes.clone(),
        allowed_tools: user
            .mcp
            .as_ref()
            .map(|m| m.allowed_tools.clone())
            .unwrap_or_default(),
    }
}

pub fn allows(user: &AuthUser, name: &str, scope: &str) -> bool {
    for_user(user).allows(name, scope)
}

pub fn ceiling(user: &AuthUser, requested: &Permissions) -> Result<(), ApiError> {
    requested.validate()?;
    if user.via_mcp && !requested.subset_of(&for_user(user)) {
        return Err(ApiError::new(StatusCode::FORBIDDEN, "permission_escalation", "Requested scopes or tools exceed this MCP grant's effective permissions. Choose a subset of its scopes/tools."));
    }
    Ok(())
}

pub fn key_permissions(
    user: &AuthUser,
    coarse: &[String],
    scopes: Option<&Vec<String>>,
    tools: Option<&Vec<String>>,
) -> Result<Option<Permissions>, ApiError> {
    if !user.via_mcp && scopes.is_none() && tools.is_none() {
        return Ok(None);
    }
    let parent = if scopes.is_none() && tools.is_none() {
        for_user(user)
    } else {
        Permissions::default()
    };
    let mut requested = Permissions {
        scopes: scopes.cloned().unwrap_or_else(|| {
            if user.via_mcp {
                parent.scopes.clone()
            } else {
                Vec::new()
            }
        }),
        allowed_tools: tools.cloned().unwrap_or_else(|| {
            if user.via_mcp {
                parent.allowed_tools.clone()
            } else {
                Vec::new()
            }
        }),
    };
    if scopes.is_some() || tools.is_some() {
        ceiling(user, &requested)?;
    }
    let write = coarse.iter().any(|s| s == "write");
    let read = coarse.iter().any(|s| s == "read");
    requested
        .scopes
        .retain(|s| if s.ends_with(":write") { write } else { read });
    requested.allowed_tools.retain(|name| {
        super::tools().iter().any(|t| {
            &t.name == name
                && if t.scope.ends_with(":write") {
                    write
                } else {
                    read
                }
        })
    });
    ceiling(user, &requested)?;
    if requested.scopes.is_empty() && requested.allowed_tools.is_empty() {
        return Err(bad(
            "The requested read/write access contains no permissions allowed by this grant.",
        ));
    }
    Ok(Some(requested))
}

#[derive(sqlx::FromRow)]
struct KeyRow {
    mcp_scopes: Option<Vec<String>>,
    mcp_allowed_tools: Option<Vec<String>>,
    created_by_mcp_grant: Option<Uuid>,
    created_by_mcp_client: Option<String>,
    parent_scopes: Option<Vec<String>>,
    parent_tools: Option<Vec<String>>,
    revoked: bool,
}

pub async fn restrict_key(
    state: &AppState,
    hash: &str,
    user: &mut AuthUser,
) -> Result<(), ApiError> {
    let row: KeyRow = sqlx::query_as("SELECT t.mcp_scopes, t.mcp_allowed_tools, t.created_by_mcp_grant, t.created_by_mcp_client, g.scopes AS parent_scopes, g.allowed_tools AS parent_tools, (t.created_by_mcp_grant IS NOT NULL AND (g.id IS NULL OR g.revoked_at IS NOT NULL OR g.created_at < now() - interval '90 days')) AS revoked FROM api_tokens t LEFT JOIN mcp_grants g ON g.id = t.created_by_mcp_grant WHERE t.token_hash = $1")
        .bind(hash).fetch_one(&state.pool).await.map_err(ApiError::database)?;
    let Some(scopes) = row.mcp_scopes else {
        return Ok(());
    };
    if row.revoked {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "grant_revoked",
            "The MCP grant that created this key has expired or been revoked.",
        ));
    }
    let saved = Permissions {
        scopes,
        allowed_tools: row.mcp_allowed_tools.unwrap_or_default(),
    };
    let parent = row.parent_scopes.map(|scopes| Permissions {
        scopes,
        allowed_tools: row.parent_tools.unwrap_or_default(),
    });
    let allowed_tools = super::tools()
        .into_iter()
        .filter(|t| {
            saved.allows(&t.name, &t.scope)
                && parent.as_ref().is_none_or(|p| p.allows(&t.name, &t.scope))
        })
        .map(|t| t.name)
        .collect();
    user.scopes.clear();
    user.via_mcp = true;
    user.mcp = Some(McpAuthority {
        grant_id: row.created_by_mcp_grant,
        client_id: row.created_by_mcp_client,
        allowed_tools,
    });
    Ok(())
}

fn bad(message: &str) -> ApiError {
    ApiError::new(StatusCode::BAD_REQUEST, "invalid_permissions", message)
}
