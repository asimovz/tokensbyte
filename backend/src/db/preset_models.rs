/*
 * tokensbyte opensource
 * (c) 2026 tokensbyte.ai
 * @copyright      Copyright netbcloud/wstianxia
 * @license        MIT (https://www.tokensbyte.ai/)
 */

//! 系统预设模型：安装时写入官方计费与转发规则已绑定的目录。

use sqlx::PgPool;

#[derive(Debug, Clone, Copy)]
pub struct PresetModelDef {
    pub mid: &'static str,
    pub name: &'static str,
    pub model_id: &'static str,
    pub original_id: &'static str,
    pub model_id_alias: &'static str,
    pub provider_name: &'static str,
    pub type_name: &'static str,
    pub billing_rule_name: &'static str,
    pub forward_rule_name: &'static str,
    pub logo: &'static str,
    pub pre_deduction: f64,
    pub feature_attributes: &'static str,
}

const VIDEO_IO: &str = r#"["文生视频","图生视频"]"#;
const VIDEO_REF: &str = r#"["文生视频","图生视频","参考生视频"]"#;

/// 与系统内置官方计费 / 转发规则名称一一对应；新增条目须新开迁移，勿改已执行过的种子。
pub fn preset_model_catalog() -> &'static [PresetModelDef] {
    &[
        PresetModelDef {
            mid: "310001",
            name: "Doubao Seedance 2.0",
            model_id: "doubao-seedance-2-0",
            original_id: "doubao-seedance-2-0",
            model_id_alias: "doubao-seedance-2-0-260128",
            provider_name: "火山引擎",
            type_name: "视频",
            billing_rule_name: "Seedance2.0官方计费",
            forward_rule_name: "火山方舟 视频生成",
            logo: "doubao",
            pre_deduction: 30.0,
            feature_attributes: VIDEO_IO,
        },
        PresetModelDef {
            mid: "310002",
            name: "Doubao Seedance 2.0 Fast",
            model_id: "doubao-seedance-2-0-fast",
            original_id: "doubao-seedance-2-0-fast",
            model_id_alias: "doubao-seedance-2-0-fast-260128",
            provider_name: "火山引擎",
            type_name: "视频",
            billing_rule_name: "Seedance2.0Fast官方计费",
            forward_rule_name: "火山方舟 视频生成",
            logo: "doubao",
            pre_deduction: 30.0,
            feature_attributes: VIDEO_IO,
        },
        PresetModelDef {
            mid: "310003",
            name: "Doubao Seedance 2.5",
            model_id: "doubao-seedance-2-5",
            original_id: "doubao-seedance-2-5",
            model_id_alias: "",
            provider_name: "火山引擎",
            type_name: "视频",
            billing_rule_name: "Seedance2.5官方计费",
            forward_rule_name: "火山方舟 视频生成",
            logo: "doubao",
            pre_deduction: 50.0,
            feature_attributes: VIDEO_IO,
        },
        PresetModelDef {
            mid: "310004",
            name: "Kling V3",
            model_id: "kling-v3",
            original_id: "kling-v3",
            model_id_alias: "",
            provider_name: "可灵 AI",
            type_name: "视频",
            billing_rule_name: "可灵V3视频计费",
            forward_rule_name: "可灵视频 3.0（文/图·推荐）",
            logo: "kling",
            pre_deduction: 8.0,
            feature_attributes: VIDEO_IO,
        },
        PresetModelDef {
            mid: "310005",
            name: "Kling V3 Omni",
            model_id: "kling-v3-omni",
            original_id: "kling-v3-omni",
            model_id_alias: "",
            provider_name: "可灵 AI",
            type_name: "视频",
            billing_rule_name: "可灵V3-Omni视频计费",
            forward_rule_name: "可灵 Omni 视频 3.0（推荐）",
            logo: "kling",
            pre_deduction: 8.0,
            feature_attributes: VIDEO_REF,
        },
        PresetModelDef {
            mid: "310006",
            name: "Kling Video O1",
            model_id: "kling-video-o1",
            original_id: "kling-video-o1",
            model_id_alias: "",
            provider_name: "可灵 AI",
            type_name: "视频",
            billing_rule_name: "可灵Video-O1视频计费",
            forward_rule_name: "可灵 Omni 视频 (kling-v3-omni/video-o1)",
            logo: "kling",
            pre_deduction: 8.0,
            feature_attributes: VIDEO_REF,
        },
        PresetModelDef {
            mid: "310007",
            name: "Kling V2.1",
            model_id: "kling-v2-1",
            original_id: "kling-v2-1",
            model_id_alias: "",
            provider_name: "可灵 AI",
            type_name: "视频",
            billing_rule_name: "可灵视频官方计费",
            forward_rule_name: "可灵 视频生成 (文/图/多图)",
            logo: "kling",
            pre_deduction: 5.0,
            feature_attributes: VIDEO_IO,
        },
    ]
}

async fn lookup_id(pool: &PgPool, sql: &str, name: &str) -> Option<i64> {
    sqlx::query_scalar::<_, i64>(sql)
        .bind(name)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
}

/// 幂等写入系统预设模型（已存在 mid 则跳过）。
pub async fn seed_system_preset_models(pool: &PgPool) -> anyhow::Result<u32> {
    let mut inserted = 0u32;
    for def in preset_model_catalog() {
        let exists: Option<i64> =
            sqlx::query_scalar("SELECT id FROM models WHERE mid = $1 LIMIT 1")
                .bind(def.mid)
                .fetch_optional(pool)
                .await?;
        if exists.is_some() {
            continue;
        }

        let provider_id = lookup_id(
            pool,
            "SELECT id FROM model_providers WHERE name = $1 LIMIT 1",
            def.provider_name,
        )
        .await;
        let type_id = lookup_id(
            pool,
            "SELECT id FROM model_types WHERE name = $1 LIMIT 1",
            def.type_name,
        )
        .await;
        let billing_rule_id = lookup_id(
            pool,
            "SELECT id FROM billing_rules WHERE name = $1 LIMIT 1",
            def.billing_rule_name,
        )
        .await;
        let forward_rule_id = lookup_id(
            pool,
            "SELECT id FROM forward_rules WHERE name = $1 LIMIT 1",
            def.forward_rule_name,
        )
        .await;

        let Some(billing_rule_id) = billing_rule_id else {
            tracing::warn!(
                "系统预设模型 {} 跳过：找不到计费规则 {}",
                def.mid,
                def.billing_rule_name
            );
            continue;
        };
        let Some(forward_rule_id) = forward_rule_id else {
            tracing::warn!(
                "系统预设模型 {} 跳过：找不到转发规则 {}",
                def.mid,
                def.forward_rule_name
            );
            continue;
        };

        let forward_ids = format!("[{forward_rule_id}]");
        let result = sqlx::query(
            r#"INSERT INTO models (
                    mid, name, model_id, original_id, model_id_alias,
                    provider_id, type_id, group_ratios, forward_rule_ids, billing_rule_id,
                    pre_deduction, site_discount, site_discount_enabled,
                    global_discount, global_discount_enabled,
                    is_active, enable_log_content, is_system, logo, remark, description,
                    feature_attributes, created_at, updated_at
               )
               SELECT $1, $2, $3, $4, $5, $6, $7, '{"default":1.0}', $8, $9,
                      $10, 1.0, 0, 1.0, 0, 1, 1, 1, $11,
                      '系统预设模型，已绑定官方计费与转发规则',
                      $12, $13, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP
               WHERE NOT EXISTS (SELECT 1 FROM models WHERE mid = $1)"#,
        )
        .bind(def.mid)
        .bind(def.name)
        .bind(def.model_id)
        .bind(def.original_id)
        .bind(def.model_id_alias)
        .bind(provider_id)
        .bind(type_id)
        .bind(&forward_ids)
        .bind(billing_rule_id)
        .bind(def.pre_deduction)
        .bind(def.logo)
        .bind(def.name)
        .bind(def.feature_attributes)
        .execute(pool)
        .await?;

        if result.rows_affected() > 0 {
            inserted += 1;
        }
    }
    Ok(inserted)
}

/// `source` 查询参数 → SQL 片段（仅允许 system/custom，避免拼接用户输入）。
pub fn source_filter_sql(source: Option<&str>) -> &'static str {
    match source {
        Some("system") => " AND is_system = 1",
        Some("custom") => " AND is_system = 0",
        _ => "",
    }
}

/// LEFT JOIN ON 条件（无前导 AND）；alias 固定为 `m`。
pub fn source_join_predicate(source: Option<&str>) -> Option<&'static str> {
    match source {
        Some("system") => Some("m.is_system = 1"),
        Some("custom") => Some("m.is_system = 0"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn catalog_mids_unique_and_stable() {
        let catalog = preset_model_catalog();
        assert!(!catalog.is_empty());
        let mut mids = HashSet::new();
        for def in catalog {
            assert!(!def.mid.is_empty());
            assert!(
                mids.insert(def.mid),
                "duplicate preset mid {}",
                def.mid
            );
            assert!(def.mid.starts_with("31"), "preset mid should use 31 prefix");
            assert!(!def.name.is_empty());
            assert!(!def.model_id.is_empty());
            assert!(!def.billing_rule_name.is_empty());
            assert!(!def.forward_rule_name.is_empty());
            assert!(!def.provider_name.is_empty());
            assert!(!def.type_name.is_empty());
        }
    }

    #[test]
    fn catalog_binds_known_official_rules() {
        let billing: HashSet<&str> = preset_model_catalog()
            .iter()
            .map(|d| d.billing_rule_name)
            .collect();
        assert!(billing.contains("Seedance2.0官方计费"));
        assert!(billing.contains("Seedance2.5官方计费"));
        assert!(billing.contains("可灵V3视频计费"));
        assert!(billing.contains("可灵视频官方计费"));

        let forwards: HashSet<&str> = preset_model_catalog()
            .iter()
            .map(|d| d.forward_rule_name)
            .collect();
        assert!(forwards.contains("火山方舟 视频生成"));
        assert!(forwards.contains("可灵视频 3.0（文/图·推荐）"));
    }

    #[test]
    fn source_filter_sql_only_known_values() {
        assert_eq!(source_filter_sql(None), "");
        assert_eq!(source_filter_sql(Some("all")), "");
        assert_eq!(source_filter_sql(Some("system")), " AND is_system = 1");
        assert_eq!(source_filter_sql(Some("custom")), " AND is_system = 0");
        assert_eq!(source_filter_sql(Some("'; drop table models --")), "");
        assert_eq!(source_join_predicate(Some("system")), Some("m.is_system = 1"));
        assert_eq!(source_join_predicate(Some("custom")), Some("m.is_system = 0"));
        assert_eq!(source_join_predicate(Some("all")), None);
        assert_eq!(source_join_predicate(None), None);
    }
}
