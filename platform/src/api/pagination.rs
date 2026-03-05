use serde::Serialize;

use crate::application::response::PaginatedResponse;

pub(crate) fn paginate_results<T: Serialize>(
    mut items: Vec<T>,
    limit: i32,
    offset: Option<i64>,
) -> PaginatedResponse<T> {
    let has_more = items.len() > limit as usize;
    if has_more {
        items.truncate(limit as usize);
    }

    PaginatedResponse {
        data: items,
        has_more,
        limit: Some(limit),
        offset: offset.map(|v| v as i32),
    }
}
