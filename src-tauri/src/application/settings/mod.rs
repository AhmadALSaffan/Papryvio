//! User settings stored locally as key/value pairs.
use crate::application::error::{AppError, AppResult};
use crate::application::views::{SettingsPatch, SettingsView};
use crate::application::Services;
use crate::domain::ids::CollectionId;

const LOCALE: &str = "locale";
const ANSWER_LOCALE: &str = "answer_locale";
const ENTER_TO_SEND: &str = "enter_to_send";
const LIBRARY_COLLAPSED: &str = "library_collapsed";
const ACTIVE_COLLECTION: &str = "active_collection_id";
pub(crate) const ONBOARDING_DISMISSED: &str = "onboarding_dismissed";

const LOCALES: [&str; 2] = ["en", "ar"];
const ANSWER_LOCALES: [&str; 3] = ["auto", "en", "ar"];

fn get_bool(svc: &Services, key: &str, default: bool) -> AppResult<bool> {
    Ok(svc.settings.get(key)?.map(|v| v == "true").unwrap_or(default))
}

pub fn get_settings(svc: &Services) -> AppResult<SettingsView> {
    let locale = svc.settings.get(LOCALE)?.filter(|l| LOCALES.contains(&l.as_str())).unwrap_or_else(|| "en".into());
    let answer_locale = svc
        .settings
        .get(ANSWER_LOCALE)?
        .filter(|l| ANSWER_LOCALES.contains(&l.as_str()))
        .unwrap_or_else(|| "auto".into());
    let mut active = svc.settings.get(ACTIVE_COLLECTION)?.and_then(|s| CollectionId::parse(&s));
    if let Some(id) = &active {
        if !svc.collections.exists(id)? {
            active = None;
        }
    }
    Ok(SettingsView {
        locale,
        answer_locale,
        enter_to_send: get_bool(svc, ENTER_TO_SEND, true)?,
        library_collapsed: get_bool(svc, LIBRARY_COLLAPSED, false)?,
        active_collection_id: active,
    })
}

pub fn update_settings(svc: &Services, patch: &SettingsPatch) -> AppResult<SettingsView> {
    let now = svc.now();
    if let Some(l) = &patch.locale {
        if !LOCALES.contains(&l.as_str()) {
            return Err(AppError::validation("unsupported locale"));
        }
        svc.settings.set(LOCALE, l, now)?;
    }
    if let Some(l) = &patch.answer_locale {
        if !ANSWER_LOCALES.contains(&l.as_str()) {
            return Err(AppError::validation("unsupported answer locale"));
        }
        svc.settings.set(ANSWER_LOCALE, l, now)?;
    }
    if let Some(v) = patch.enter_to_send {
        svc.settings.set(ENTER_TO_SEND, if v { "true" } else { "false" }, now)?;
    }
    if let Some(v) = patch.library_collapsed {
        svc.settings.set(LIBRARY_COLLAPSED, if v { "true" } else { "false" }, now)?;
    }
    if let Some(raw) = &patch.active_collection_id {
        if raw.is_empty() {
            svc.settings.set(ACTIVE_COLLECTION, "", now)?;
        } else {
            let id = CollectionId::parse(raw).ok_or_else(|| AppError::validation("bad collection id"))?;
            if !svc.collections.exists(&id)? {
                return Err(AppError::not_found("collection"));
            }
            svc.settings.set(ACTIVE_COLLECTION, id.as_str(), now)?;
        }
    }
    get_settings(svc)
}

pub fn clear_active_collection(svc: &Services) -> AppResult<()> {
    svc.settings.set(ACTIVE_COLLECTION, "", svc.now())
}
