/*
 * tokensbyte opensource
 * (c) 2026 tokensbyte.ai
 * @copyright      Copyright netbcloud/wstianxia
 * @license        MIT (https://www.tokensbyte.ai/)
 */

pub mod affiliate;
pub mod email;
pub mod http_client;
pub mod notification;
pub mod oauth;
#[cfg(plugin_payment)]
pub mod payment;
pub mod runtime_info;
pub mod sms;
pub mod tos;
pub mod upstream_rate_sync;
// 上游素材透传客户端：ark_asset_proxy uar: 分支与 asset_convert 依赖，不再受商业 feature 门控
pub mod upstream_asset_client;
#[cfg(feature = "commercial_plugins")]
pub mod volc_ark_monitor;
pub mod volcengine;
