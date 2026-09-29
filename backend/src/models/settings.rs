/*
 * tokensbyte opensource
 * (c) 2026 tokensbyte.ai
 * @copyright      Copyright netbcloud/wstianxia
 * @license        MIT (https://www.tokensbyte.ai/)
 */

use serde::{Deserialize, Serialize};

/// 站点基本信息设置
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct SiteSettings {
    #[serde(default = "default_site_name")]
    pub name: String,
    #[serde(default = "default_site_name")]
    pub title: String,
    #[serde(default)]
    pub keywords: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub favicon: String,
    #[serde(default)]
    pub logo: String,
    /// 控制台 Logo/站点名点击跳转地址（留空则不可点击）
    #[serde(default)]
    pub logo_title_url: String,
    #[serde(default)]
    pub login_title: String,
    /// 登录页标题点击跳转地址（留空则回退到 logo_title_url，仍空则不可点击）
    #[serde(default)]
    pub login_title_url: String,
    #[serde(default)]
    pub login_subtitle: String,
    #[serde(default = "default_enable_multilingual")]
    pub enable_multilingual: bool,
    /// 站点支持的语言列表（语言代码），如 ["zh", "en"]
    #[serde(default = "default_supported_languages")]
    pub supported_languages: Vec<String>,
    /// 站点默认语言
    #[serde(default = "default_language")]
    pub default_language: String,
    /// 站点默认时区
    #[serde(default = "default_site_timezone")]
    pub default_timezone: String,
    /// 是否在前端显示时区后缀
    #[serde(default = "default_show_timezone")]
    pub show_timezone: bool,
    /// 是否允许用户切换亮色/暗色主题（关闭后用户端不显示切换按钮）
    #[serde(default = "default_true_theme")]
    pub enable_theme_toggle: bool,
    /// 站点默认主题："dark" 或 "light"
    #[serde(default = "default_theme_mode")]
    pub default_theme: String,
    /// 版权信息，显示在登录页面底部
    #[serde(default = "default_copyright")]
    pub copyright: String,
    /// 管理后台访问路径，默认 admin1688
    #[serde(default = "default_admin_path")]
    pub admin_path: String,
    /// 登录页风格："split"（左右风格）或 "classic"（经典风格）
    #[serde(default = "default_login_style")]
    pub login_style: String,
    /// 左右风格下的左侧广告语名言
    #[serde(default)]
    pub login_quote: String,
    /// 是否开启注册 IP 黑名单拦截
    #[serde(default)]
    pub ip_blacklist_enabled: bool,
    /// 注册 IP 黑名单列表 (支持单 IP 及 CIDR 网段)
    #[serde(default)]
    pub ip_blacklist: Vec<String>,
}

fn default_login_style() -> String {
    "split".to_string()
}

fn default_admin_path() -> String {
    "admin1688".to_string()
}

fn default_copyright() -> String {
    "© 2026 TkeAPI. All rights reserved.".to_string()
}

fn default_site_name() -> String {
    "Tkeapi".to_string()
}

fn default_true_theme() -> bool {
    true
}

fn default_theme_mode() -> String {
    "dark".to_string()
}

fn default_enable_multilingual() -> bool {
    true
}

fn default_supported_languages() -> Vec<String> {
    vec!["zh".to_string(), "en".to_string()]
}

fn default_language() -> String {
    "zh".to_string()
}

fn default_site_timezone() -> String {
    iana_time_zone::get_timezone()
        .unwrap_or_else(|_| crate::time_system::DEFAULT_TIMEDISPLAY.to_string())
}

fn default_show_timezone() -> bool {
    true
}

/// 站点协议设置
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct AgreementSettings {
    #[serde(default = "default_agreement_mode")]
    pub tos_mode: String, // "text" or "link"
    #[serde(default = "default_agreement_mode")]
    pub tos_mode_en: String,
    #[serde(default)]
    pub tos_content: String,
    #[serde(default)]
    pub tos_content_en: String,
    #[serde(default)]
    pub tos_link: String,
    #[serde(default)]
    pub tos_link_en: String,
    #[serde(default = "default_agreement_mode")]
    pub privacy_mode: String, // "text" or "link"
    #[serde(default = "default_agreement_mode")]
    pub privacy_mode_en: String,
    #[serde(default)]
    pub privacy_content: String,
    #[serde(default)]
    pub privacy_content_en: String,
    #[serde(default)]
    pub privacy_link: String,
    #[serde(default)]
    pub privacy_link_en: String,
    #[serde(default)]
    pub tos_enabled: bool,
    #[serde(default)]
    pub privacy_enabled: bool,
}

fn default_agreement_mode() -> String {
    "link".to_string()
}

/// 辅助货币设置
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AuxiliaryCurrency {
    #[serde(default)]
    pub code: String, // e.g., "USD"
    #[serde(default)]
    pub symbol: String, // e.g., "$"
    #[serde(default)]
    pub exchange_rate: f64, // e.g., if default is CNY and this is USD, rate could be 0.14
    #[serde(default = "default_true")]
    pub enabled: bool,
}

/// 货币设置
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct CurrencySettings {
    #[serde(default)]
    pub default_currency: String,
    #[serde(default)]
    pub currency_symbol: String,
    #[serde(default)]
    pub currency_unit: String,
    #[serde(default)]
    pub token_ratio: f64,
    #[serde(default)]
    pub auxiliary_currencies: Vec<AuxiliaryCurrency>,
    #[serde(default = "default_quick_amounts")]
    pub quick_amounts: Vec<f64>,
    #[serde(default = "default_min_recharge_amount")]
    pub min_recharge_amount: f64,
}

fn default_quick_amounts() -> Vec<f64> {
    vec![20.0, 50.0, 100.0, 500.0, 1000.0, 5000.0]
}

fn default_min_recharge_amount() -> f64 {
    5.0
}

/// 登录方式设置 — 控制用户端可用的登录方式
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct LoginSettings {
    #[serde(default = "default_true")]
    pub enable_username_login: bool,
    #[serde(default)]
    pub enable_mobile_login: bool,
    #[serde(default)]
    pub enable_email_login: bool,
    #[serde(default)]
    pub enable_wechat_login: bool,
    #[serde(default)]
    pub enable_google_login: bool,
}

/// 注册方式设置 — 控制用户端可用的注册方式及安全策略
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct RegistrationSettings {
    #[serde(default)]
    pub enable_username_registration: bool,
    #[serde(default)]
    pub enable_email_registration: bool,
    #[serde(default)]
    pub enable_mobile_registration: bool,
    #[serde(default)]
    pub enable_password_recovery: bool,
    /// IP 防刷：开启后限制同 IP 每日注册次数
    #[serde(default)]
    pub ip_rate_limit_enabled: bool,
    /// 同 IP 每日最多注册数
    #[serde(default = "default_ip_daily_limit")]
    pub ip_daily_limit: i32,
    /// 邮箱防刷：开启后 @ 前仅允许数字+字母+"_"，长度≤25
    #[serde(default)]
    pub email_validation_strict: bool,
    /// 邮箱白名单：开启后仅允许指定域名邮箱注册
    #[serde(default)]
    pub email_whitelist_enabled: bool,
    /// 允许的邮箱域名列表
    #[serde(default = "default_email_whitelist")]
    pub email_whitelist: Vec<String>,
    /// 是否要求绑定手机号（纳入绑定策略）
    #[serde(default)]
    pub require_bind_mobile: bool,
    /// 是否要求绑定邮箱（纳入绑定策略）
    #[serde(default)]
    pub require_bind_email: bool,
    /// 绑定执行方式：all=全部都要 / any=满足其一 / prompt_only=仅弹窗提示
    #[serde(default = "default_bind_enforcement")]
    pub bind_enforcement: String,
    /// 是否开启站点用户实名认证（KYC）
    #[serde(default)]
    pub enable_user_kyc: bool,
}

fn default_bind_enforcement() -> String {
    "all".to_string()
}

impl RegistrationSettings {
    /// 是否启用了任一绑定通道
    pub fn bind_policy_active(&self) -> bool {
        self.require_bind_mobile || self.require_bind_email
    }

    pub fn has_valid_mobile(mobile: Option<&str>) -> bool {
        mobile.map(|m| !m.trim().is_empty()).unwrap_or(false)
    }

    pub fn has_valid_email(email: &str) -> bool {
        let email = email.trim();
        !email.is_empty() && !email.ends_with("@tokensbyte.local")
    }

    /// 用户当前绑定是否满足策略（与执行方式无关；prompt_only 也用此判断是否弹窗）
    pub fn is_bind_satisfied(&self, email: &str, mobile: Option<&str>) -> bool {
        if !self.bind_policy_active() {
            return true;
        }
        let has_mobile = Self::has_valid_mobile(mobile);
        let has_email = Self::has_valid_email(email);
        match (self.require_bind_mobile, self.require_bind_email) {
            (true, true) => {
                if self.bind_enforcement == "any" {
                    has_mobile || has_email
                } else {
                    // all / prompt_only：双开时按「都要」判断是否已满足
                    has_mobile && has_email
                }
            }
            (true, false) => has_mobile,
            (false, true) => has_email,
            (false, false) => true,
        }
    }

    /// 创建令牌等敏感操作是否应硬拦截
    pub fn should_block_token_create(&self, email: &str, mobile: Option<&str>) -> bool {
        self.bind_policy_active()
            && self.bind_enforcement != "prompt_only"
            && !self.is_bind_satisfied(email, mobile)
    }

    /// 生成创建令牌失败时的提示文案
    pub fn token_bind_block_message(&self) -> String {
        match (self.require_bind_mobile, self.require_bind_email) {
            (true, true) if self.bind_enforcement == "any" => {
                "创建 API 令牌前请先绑定手机号或邮箱".to_string()
            }
            (true, true) => "创建 API 令牌前请先绑定手机号和邮箱".to_string(),
            (true, false) => "创建 API 令牌前请先绑定手机号".to_string(),
            (false, true) => "创建 API 令牌前请先绑定邮箱".to_string(),
            _ => "创建 API 令牌前请先完成账号绑定".to_string(),
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_ip_daily_limit() -> i32 {
    6
}

fn default_email_whitelist() -> Vec<String> {
    vec![
        "qq.com".to_string(),
        "163.com".to_string(),
        "outlook.com".to_string(),
        "aliyun.com".to_string(),
        "foxmail.com".to_string(),
    ]
}

/// SMTP 邮箱通知设置
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct SMTPSettings {
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub port: u16,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub from_address: String,
    #[serde(default)]
    pub from_name: String,
}

/// 短信通知设置（provider: tencent | volcengine；缺省 tencent 兼容旧配置）
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SmsSettings {
    /// 服务商：tencent（腾讯云）| volcengine（火山引擎）
    #[serde(default = "default_sms_provider")]
    pub provider: String,
    /// 腾讯云 SecretId / 火山 Access Key
    #[serde(default)]
    pub secret_id: String,
    /// 腾讯云 SecretKey / 火山 Secret Key
    #[serde(default)]
    pub secret_key: String,
    /// 腾讯云 SdkAppId / 火山消息组 ID（SmsAccount）
    #[serde(default)]
    pub sdk_app_id: String,
    /// 已审核的短信签名
    #[serde(default)]
    pub sign_name: String,
    /// 验证码模板 ID
    #[serde(default)]
    pub template_id: String,
    /// 余额不足提醒模板 ID（无变量固定正文）
    #[serde(default)]
    pub balance_template_id: String,
    /// 火山验证码模板变量名（TemplateParam JSON 键，默认 code）
    #[serde(default = "default_sms_code_param")]
    pub code_param: String,
}

fn default_sms_provider() -> String {
    "tencent".to_string()
}

fn default_sms_code_param() -> String {
    "code".to_string()
}

impl Default for SmsSettings {
    fn default() -> Self {
        Self {
            provider: default_sms_provider(),
            secret_id: String::new(),
            secret_key: String::new(),
            sdk_app_id: String::new(),
            sign_name: String::new(),
            template_id: String::new(),
            balance_template_id: String::new(),
            code_param: default_sms_code_param(),
        }
    }
}

impl SmsSettings {
    pub fn is_volcengine(&self) -> bool {
        self.provider.trim().eq_ignore_ascii_case("volcengine")
    }

    pub fn credentials_configured(&self) -> bool {
        !self.secret_id.trim().is_empty() && !self.secret_key.trim().is_empty()
    }

    /// 是否已配置余额提醒模板（开启短信余额提醒前的前置条件）
    pub fn balance_template_configured(&self) -> bool {
        !self.balance_template_id.trim().is_empty()
    }

    /// 发送时使用的余额模板 ID（去空白）
    pub fn balance_template_id_effective(&self) -> &str {
        self.balance_template_id.trim()
    }

    /// 火山验证码变量名（与控制台变量名一致，如 1 / code，勿带 ${}）
    pub fn code_param_effective(&self) -> &str {
        let p = self.code_param.trim();
        if p.is_empty() {
            "code"
        } else {
            p
        }
    }
}

/// 营销设置
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct MarketingSettings {
    #[serde(default)]
    pub enable_registration_gift: bool,
    /// 是否开启用户端兑换码功能
    #[serde(default)]
    pub enable_redemption: bool,
    #[serde(default = "default_gift_mode")]
    pub gift_mode: String, // "fixed" or "random"
    #[serde(default)]
    pub fixed_amount: f64,
    #[serde(default)]
    pub min_amount: f64,
    #[serde(default)]
    pub max_amount: f64,
}

fn default_gift_mode() -> String {
    "fixed".to_string()
}

/// 数据库连接设置
#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
pub struct DatabaseSettings {
    #[serde(default)]
    pub db_type: String, // "postgres"
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub port: u16,
    #[serde(default)]
    pub database: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub ssl_mode: bool,
}

/// 微信支付设置
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct PaymentWechatSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub mchid: String,
    #[serde(default)]
    pub appid: String,
    #[serde(default)]
    pub api_v3_key: String,
    #[serde(default)]
    pub cert_serial_no: String,
    #[serde(default)]
    pub private_key: String,
}

/// 支付宝设置
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct PaymentAlipaySettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub app_id: String,
    #[serde(default)]
    pub private_key: String,
    #[serde(default)]
    pub alipay_public_key: String,
    #[serde(default = "default_sign_type")]
    pub sign_type: String,
}

fn default_sign_type() -> String {
    "RSA2".to_string()
}

/// Stripe 支付设置
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct PaymentStripeSettings {
    #[serde(default)]
    pub enabled: bool,
    /// Stripe Secret Key (sk_live_xxx 或 sk_test_xxx)
    #[serde(default)]
    pub secret_key: String,
    /// Stripe Publishable Key (pk_live_xxx 或 pk_test_xxx)
    #[serde(default)]
    pub publishable_key: String,
    /// Stripe Webhook Signing Secret (whsec_xxx)
    #[serde(default)]
    pub webhook_secret: String,
}

/// BonusPay 加密货币支付设置
/// 基于 https://docs.bonuspay.network 文档
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct PaymentBonuspaySettings {
    #[serde(default)]
    pub enabled: bool,
    /// BonusPay 商户 Partner-Id (如 200000000888)
    #[serde(default)]
    pub partner_id: String,
    /// 商户 RSA 私钥 (PKCS#8 PEM 格式，用于请求签名)
    #[serde(default)]
    pub merchant_private_key: String,
    /// BonusPay RSA 公钥 (PEM 格式，用于验证回调签名)
    #[serde(default)]
    pub bonuspay_public_key: String,
    /// API 接口地址
    #[serde(default = "default_bonuspay_api_url")]
    pub api_url: String,
    /// USDT/USDC 兑换系统货币(如CNY)的汇率
    #[serde(default = "default_crypto_exchange_rate")]
    pub crypto_exchange_rate: f64,
}

fn default_crypto_exchange_rate() -> f64 {
    1.0
}

fn default_bonuspay_api_url() -> String {
    "https://api.bonuspay.network".to_string()
}

/// HyperBC 加密货币支付设置
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct PaymentHyperbcSettings {
    #[serde(default)]
    pub enabled: bool,
    /// CipherBC 分配的 APP_ID
    #[serde(default)]
    pub app_id: String,
    /// 商户 RSA 私钥 (PEM 格式，用于请求签名)
    #[serde(default)]
    pub merchant_private_key: String,
    /// CipherBC 平台 RSA 公钥 (PEM 格式，用于验证回调签名)
    #[serde(default)]
    pub hyperbc_public_key: String,
    /// API 接口地址
    #[serde(default = "default_hyperbc_api_url")]
    pub api_url: String,
    /// USDT/加密货币 兑换系统货币的汇率
    #[serde(default = "default_crypto_exchange_rate")]
    pub crypto_exchange_rate: f64,
}

fn default_hyperbc_api_url() -> String {
    "https://api.hyperbc.com".to_string()
}

/// 通联支付设置
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct PaymentAllinpaySettings {
    /// 是否启用通联支付方式
    #[serde(default)]
    pub enabled: bool,
    /// 实际交易商户号 (cusid)
    #[serde(default)]
    pub cusid: String,
    /// 平台分配的应用ID (appid)
    #[serde(default)]
    pub appid: String,
    /// 商户 RSA 私钥 (PKCS#1 Base64/PEM，对应通联「RSA公钥」栏位上传的商户公钥)
    #[serde(default)]
    pub merchant_private_key: String,
    /// 通联平台 RSA 公钥 (PEM，填商服「通联RSA公钥」，用于回调/查询验签)
    #[serde(default)]
    pub allinpay_public_key: String,
    /// 签名类型固定为 RSA（SHA1WithRSA）；保留字段以兼容已存配置
    #[serde(default = "default_allinpay_sign_type")]
    pub sign_type: String,
    /// 接口网关地址
    #[serde(default = "default_allinpay_api_url")]
    pub api_url: String,
    /// 统一支付业务接口协议版本
    #[serde(default = "default_allinpay_version")]
    pub version: String,
}

fn default_allinpay_api_url() -> String {
    "https://vsp.allinpay.com/apiweb".to_string()
}

fn default_allinpay_version() -> String {
    "11".to_string()
}

fn default_allinpay_sign_type() -> String {
    "RSA".to_string()
}

/// 支付渠道用户端展示配置（与各 payment_* 密钥配置解耦）
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct PaymentChannelsUiSettings {
    #[serde(default)]
    pub channels: Vec<PaymentChannelUiItem>,
}

/// 单个支付渠道的展示与排序
///
/// - 普通渠道 id 即用户端 payment_method（如 alipay / wechat）
/// - 通联聚合渠道 id 为 `allinpay`；实际下单仍用 `allinpay_wechat` / `allinpay_alipay`
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct PaymentChannelUiItem {
    #[serde(default)]
    pub id: String,
    /// 排序权重，数字越大越靠前
    #[serde(default)]
    pub sort_order: i32,
    #[serde(default)]
    pub enabled: bool,
    /// 用户端中文显示名称；空则使用系统默认
    #[serde(default)]
    pub display_name: Option<String>,
    /// 用户端英文显示名称；空则使用系统默认。非中文站点语言使用此字段
    #[serde(default)]
    pub display_name_en: Option<String>,
    /// 用户端中文副标题/角标；空则使用系统默认
    #[serde(default)]
    pub subtitle: Option<String>,
    /// 用户端英文副标题/角标；空则使用系统默认
    #[serde(default)]
    pub subtitle_en: Option<String>,
    /// Logo 图片 URL；空则使用系统默认图标
    #[serde(default)]
    pub logo_url: Option<String>,
    /// 通联子渠道：微信（仅 id=allinpay 有效）
    #[serde(default = "default_true")]
    pub allinpay_wechat_enabled: bool,
    /// 通联子渠道：支付宝（仅 id=allinpay 有效）
    #[serde(default = "default_true")]
    pub allinpay_alipay_enabled: bool,
}

/// 公开支付渠道（不含密钥）
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct PublicPaymentChannel {
    pub id: String,
    pub sort_order: i32,
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name_en: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitle_en: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logo_url: Option<String>,
    /// 通联已开启的子渠道 payment_method 列表（如 allinpay_wechat）
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allinpay_methods: Vec<String>,
}

/// 系统内置支付渠道 ID 与默认排序（越大越靠前）
pub fn payment_channel_catalog() -> &'static [(&'static str, i32)] {
    &[
        ("alipay", 70),
        ("wechat", 60),
        ("allinpay", 50),
        ("stripe", 30),
        ("bonuspay", 20),
        ("hyperbc", 10),
    ]
}

fn trim_opt(s: &Option<String>) -> Option<String> {
    s.as_ref()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

impl PaymentChannelUiItem {
    pub fn normalized_display(&self) -> Option<String> {
        trim_opt(&self.display_name)
    }

    pub fn normalized_display_en(&self) -> Option<String> {
        trim_opt(&self.display_name_en)
    }

    pub fn normalized_subtitle(&self) -> Option<String> {
        trim_opt(&self.subtitle)
    }

    pub fn normalized_subtitle_en(&self) -> Option<String> {
        trim_opt(&self.subtitle_en)
    }

    pub fn normalized_logo_url(&self) -> Option<String> {
        trim_opt(&self.logo_url)
    }

    pub fn allinpay_methods(&self) -> Vec<String> {
        let mut methods = Vec::new();
        if self.allinpay_wechat_enabled {
            methods.push("allinpay_wechat".to_string());
        }
        if self.allinpay_alipay_enabled {
            methods.push("allinpay_alipay".to_string());
        }
        methods
    }
}

/// 用已存配置合并内置渠道目录；兼容旧版拆分的 allinpay_wechat / allinpay_alipay
pub fn merge_payment_channels_ui(
    saved: Option<PaymentChannelsUiSettings>,
    gateway: &PaymentGatewayEnableFlags,
) -> PaymentChannelsUiSettings {
    let mut by_id: std::collections::HashMap<String, PaymentChannelUiItem> = saved
        .unwrap_or_default()
        .channels
        .into_iter()
        .filter(|c| !c.id.trim().is_empty())
        .map(|c| (c.id.clone(), c))
        .collect();

    // 旧数据：两个通联子渠道 → 合并为 allinpay
    let legacy_wechat = by_id.remove("allinpay_wechat");
    let legacy_alipay = by_id.remove("allinpay_alipay");
    if !by_id.contains_key("allinpay") && (legacy_wechat.is_some() || legacy_alipay.is_some()) {
        let lw = legacy_wechat.as_ref();
        let la = legacy_alipay.as_ref();
        let sort_order = lw
            .map(|c| c.sort_order)
            .into_iter()
            .chain(la.map(|c| c.sort_order))
            .max()
            .unwrap_or(50);
        let enabled =
            lw.map(|c| c.enabled).unwrap_or(false) || la.map(|c| c.enabled).unwrap_or(false);
        let display = lw
            .and_then(|c| c.normalized_display())
            .or_else(|| la.and_then(|c| c.normalized_display()));
        let display_en = lw
            .and_then(|c| c.normalized_display_en())
            .or_else(|| la.and_then(|c| c.normalized_display_en()));
        let subtitle = lw
            .and_then(|c| c.normalized_subtitle())
            .or_else(|| la.and_then(|c| c.normalized_subtitle()));
        let subtitle_en = lw
            .and_then(|c| c.normalized_subtitle_en())
            .or_else(|| la.and_then(|c| c.normalized_subtitle_en()));
        let logo = lw
            .and_then(|c| c.normalized_logo_url())
            .or_else(|| la.and_then(|c| c.normalized_logo_url()));
        by_id.insert(
            "allinpay".to_string(),
            PaymentChannelUiItem {
                id: "allinpay".to_string(),
                sort_order,
                enabled,
                display_name: display,
                display_name_en: display_en,
                subtitle,
                subtitle_en,
                logo_url: logo,
                allinpay_wechat_enabled: lw.map(|c| c.enabled).unwrap_or(gateway.allinpay),
                allinpay_alipay_enabled: la.map(|c| c.enabled).unwrap_or(gateway.allinpay),
            },
        );
    }

    let mut channels = Vec::with_capacity(payment_channel_catalog().len());
    for &(id, default_sort) in payment_channel_catalog() {
        if let Some(mut item) = by_id.remove(id) {
            if item.id.is_empty() {
                item.id = id.to_string();
            }
            if id == "allinpay"
                && !item.allinpay_wechat_enabled
                && !item.allinpay_alipay_enabled
                && item.enabled
            {
                // 开启主开关但未指定子渠道时，默认两边都开
                item.allinpay_wechat_enabled = true;
                item.allinpay_alipay_enabled = true;
            }
            channels.push(item);
        } else {
            let gateway_on = gateway.default_enabled_for(id);
            channels.push(PaymentChannelUiItem {
                id: id.to_string(),
                sort_order: default_sort,
                enabled: gateway_on,
                display_name: None,
                display_name_en: None,
                subtitle: None,
                subtitle_en: None,
                logo_url: None,
                allinpay_wechat_enabled: true,
                allinpay_alipay_enabled: true,
            });
        }
    }
    PaymentChannelsUiSettings { channels }
}

/// 各网关当前启用状态（用于补齐渠道默认 enabled）
#[derive(Debug, Clone, Default)]
pub struct PaymentGatewayEnableFlags {
    pub wechat: bool,
    pub alipay: bool,
    pub stripe: bool,
    pub bonuspay: bool,
    pub hyperbc: bool,
    pub allinpay: bool,
}

impl PaymentGatewayEnableFlags {
    pub fn default_enabled_for(&self, channel_id: &str) -> bool {
        match channel_id {
            "wechat" => self.wechat,
            "alipay" => self.alipay,
            "stripe" => self.stripe,
            "bonuspay" => self.bonuspay,
            "hyperbc" => self.hyperbc,
            "allinpay" => self.allinpay,
            _ => false,
        }
    }

    pub fn gateway_ready_for(&self, channel_id: &str) -> bool {
        self.default_enabled_for(channel_id)
    }
}

/// 计算公开渠道列表：渠道 enabled ∧ 对应网关 enabled
pub fn build_public_payment_channels(
    ui: &PaymentChannelsUiSettings,
    gateway: &PaymentGatewayEnableFlags,
) -> Vec<PublicPaymentChannel> {
    let mut list: Vec<PublicPaymentChannel> = ui
        .channels
        .iter()
        .map(|c| {
            let gateway_on = gateway.gateway_ready_for(&c.id);
            let methods = if c.id == "allinpay" {
                c.allinpay_methods()
            } else {
                Vec::new()
            };
            let channel_on = if c.id == "allinpay" {
                c.enabled && !methods.is_empty()
            } else {
                c.enabled
            };
            PublicPaymentChannel {
                id: c.id.clone(),
                sort_order: c.sort_order,
                enabled: channel_on && gateway_on,
                display_name: c.normalized_display(),
                display_name_en: c.normalized_display_en(),
                subtitle: c.normalized_subtitle(),
                subtitle_en: c.normalized_subtitle_en(),
                logo_url: c.normalized_logo_url(),
                allinpay_methods: if c.id == "allinpay" && gateway_on && c.enabled {
                    methods
                } else {
                    Vec::new()
                },
            }
        })
        .collect();
    list.sort_by(|a, b| {
        b.sort_order
            .cmp(&a.sort_order)
            .then_with(|| a.id.cmp(&b.id))
    });
    list
}

/// 从公开渠道列表推导兼容旧字段的 PublicPaymentStatus
pub fn public_payment_status_from_channels(
    channels: &[PublicPaymentChannel],
) -> PublicPaymentStatus {
    let on = |id: &str| channels.iter().any(|c| c.id == id && c.enabled);
    PublicPaymentStatus {
        wechat_enabled: on("wechat"),
        alipay_enabled: on("alipay"),
        stripe_enabled: on("stripe"),
        bonuspay_enabled: on("bonuspay"),
        hyperbc_enabled: on("hyperbc"),
        allinpay_enabled: on("allinpay"),
    }
}

/// 查询某下单 payment_method 在 UI 配置中是否允许
///
/// - 普通渠道：对应 id 的 enabled
/// - 通联子渠道：优先读聚合渠道 `allinpay` 的子开关；兼容旧版拆分 id
/// - 配置完全缺失时默认 true（由网关 enabled 兜底，兼容未迁移数据）
pub fn is_payment_channel_ui_enabled(ui: &PaymentChannelsUiSettings, channel_id: &str) -> bool {
    if channel_id == "allinpay_wechat" || channel_id == "allinpay_alipay" {
        if let Some(c) = ui.channels.iter().find(|c| c.id == "allinpay") {
            return c.enabled
                && if channel_id == "allinpay_wechat" {
                    c.allinpay_wechat_enabled
                } else {
                    c.allinpay_alipay_enabled
                };
        }
        // 兼容尚未迁移的拆分配置
        if let Some(c) = ui.channels.iter().find(|c| c.id == channel_id) {
            return c.enabled;
        }
        return true;
    }

    ui.channels
        .iter()
        .find(|c| c.id == channel_id)
        .map(|c| c.enabled)
        .unwrap_or(true)
}

/// 谷歌 OAuth 2.0 设置
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct GoogleOAuthSettings {
    /// Google OAuth Client ID
    #[serde(default)]
    pub client_id: String,
    /// Google OAuth Client Secret
    #[serde(default)]
    pub client_secret: String,
}

/// 微信开放平台授权登录设置
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct WechatOAuthSettings {
    /// 网站应用 AppId
    #[serde(default)]
    pub app_id: String,
    /// 网站应用密钥 AppSecret
    #[serde(default)]
    pub app_secret: String,
}

/// 低余额视频在途档位：可用额低于 max_available 时限制未完成视频路数；max_available=None 表示其余
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VideoInflightTier {
    /// 可用额低于该值时命中本档；None = 其余档
    #[serde(default)]
    pub max_available: Option<f64>,
    /// 最大未完成视频路数；**0 = 不限制**
    #[serde(default)]
    pub max_inflight: u32,
}

impl VideoInflightTier {
    fn cmp_avail(a: &Self, b: &Self) -> std::cmp::Ordering {
        match (a.max_available, b.max_available) {
            (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        }
    }
}

fn default_video_inflight_tiers() -> Vec<VideoInflightTier> {
    // 仅限制低余额档；未命中 = 不限制
    vec![
        VideoInflightTier {
            max_available: Some(20.0),
            max_inflight: 1,
        },
        VideoInflightTier {
            max_available: Some(50.0),
            max_inflight: 3,
        },
    ]
}

/// Relay 网关设置（管理端；不进公开接口）
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RelaySettings {
    /// 手动轮询是否打上游。关：优先 logs，无缓存再兜底；后台自动轮询不受影响。
    #[serde(default = "default_true")]
    pub manual_poll_upstream: bool,
    /// 后台 TaskPoller 周期（秒）；过短增上游压力，过长延迟结案/退费。默认 30，有效范围 5–300；缺省/0 按 30。
    #[serde(default)]
    pub poll_tick_secs: u64,
    /// 低余额限制未完成视频路数；默认关
    #[serde(default)]
    pub video_inflight_enabled: bool,
    /// 按可用额落档的在途上限（空则回落默认档）
    #[serde(default = "default_video_inflight_tiers")]
    pub video_inflight_tiers: Vec<VideoInflightTier>,
    /// 素材上游按渠道分组推导。开：先按「等级可达分组 → 分组凭证 → 素材绑定」定位，
    /// 推不出时回落等级映射；关（默认）：仅按【上游素材绑定】的用户等级映射定位，与改造前一致。
    #[serde(default)]
    pub asset_route_by_group: bool,
}

impl RelaySettings {
    /// 入缓存前：空档补默认，按可用额升序（无上限档置末）；轮询周期 0→30 再钳 5–300
    pub fn prepared(mut self) -> Self {
        self.poll_tick_secs = match self.poll_tick_secs {
            0 => 30,
            n => n.clamp(5, 300),
        };
        if self.video_inflight_tiers.is_empty() {
            self.video_inflight_tiers = default_video_inflight_tiers();
        }
        self.video_inflight_tiers
            .sort_by(VideoInflightTier::cmp_avail);
        self
    }

    /// 当前可用额下的在途上限；未启用或该档 `max_inflight=0` → `None`（不限制）
    pub fn max_video_inflight(&self, available: f64) -> Option<u32> {
        if !self.video_inflight_enabled {
            return None;
        }
        for t in &self.video_inflight_tiers {
            let hit = match t.max_available {
                Some(limit) => available < limit,
                None => true,
            };
            if hit {
                return (t.max_inflight > 0).then_some(t.max_inflight);
            }
        }
        None
    }
}

impl Default for RelaySettings {
    fn default() -> Self {
        Self {
            manual_poll_upstream: true,
            poll_tick_secs: 30,
            video_inflight_enabled: false,
            video_inflight_tiers: default_video_inflight_tiers(),
            asset_route_by_group: false,
        }
    }
}

/// 存储配置
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct StorageSettings {
    #[serde(default)]
    pub tos_access_key: String,
    #[serde(default)]
    pub tos_secret_key: String,
    #[serde(default)]
    pub tos_endpoint: String,
    #[serde(default)]
    pub tos_region: String,
    #[serde(default)]
    pub tos_bucket: String,
    #[serde(default)]
    pub tos_path_prefix: String,
    #[serde(default)]
    pub tos_custom_domain: String,
    /// 使用日志详情保留天数，超期自动清理请求/响应内容，0=永不清理
    #[serde(default = "default_log_retention_days")]
    pub log_retention_days: i32,
    /// 使用日志行保留天数：超期行迁入 logs_archive 并从热表删除；0=永不归档（默认）
    /// 建议 ≥ 详情保留天数，且先确保 usage_daily_stats 已覆盖对应日期。
    #[serde(default = "default_log_row_retention_days")]
    pub log_row_retention_days: i32,
    /// 火山素材自动清理保留天数（转发转换缓存 + 上游转素材缓存）；缺省 7；0=关闭。不含 api_proxy。
    #[serde(default = "default_volc_asset_retention_days")]
    pub volc_asset_retention_days: i32,
}

impl Default for StorageSettings {
    fn default() -> Self {
        Self {
            tos_access_key: String::new(),
            tos_secret_key: String::new(),
            tos_endpoint: String::new(),
            tos_region: String::new(),
            tos_bucket: String::new(),
            tos_path_prefix: String::new(),
            tos_custom_domain: String::new(),
            log_retention_days: default_log_retention_days(),
            log_row_retention_days: default_log_row_retention_days(),
            volc_asset_retention_days: default_volc_asset_retention_days(),
        }
    }
}

fn default_log_retention_days() -> i32 {
    30
}

fn default_log_row_retention_days() -> i32 {
    0
}

fn default_volc_asset_retention_days() -> i32 {
    30
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MenuItemConfig {
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub label_zh: String,
    #[serde(default)]
    pub label_en: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub sort_order: i32,
    #[serde(default = "default_all_levels")]
    pub allowed_levels: String,
}

fn default_all_levels() -> String {
    "all".to_string()
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct MenuConfigSettings {
    #[serde(default)]
    pub items: Vec<MenuItemConfig>,
}

/// 提示通知设置
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NotificationSettings {
    #[serde(default)]
    pub site_notification_enabled: bool,
    /// 是否向用户开放短信余额提醒订阅
    #[serde(default)]
    pub sms_balance_notification: bool,
    /// 是否向用户开放邮件余额提醒订阅
    #[serde(default)]
    pub email_balance_notification: bool,
    /// 是否向用户开放站内 Web 通知订阅
    #[serde(default = "default_true_notif")]
    pub web_notification_enabled: bool,
    /// 是否向用户开放浏览器 Push 订阅
    #[serde(default = "default_true_notif")]
    pub push_notification_enabled: bool,
    /// 是否向用户开放勿扰模式
    #[serde(default = "default_true_notif")]
    pub do_not_disturb_enabled: bool,
    #[serde(default = "default_low_balance_threshold")]
    pub low_balance_threshold: f64,
    /// 余额不足提醒邮件主题（支持 {{site_name}} {{balance}} {{threshold}}）
    #[serde(default = "default_low_balance_email_subject")]
    pub low_balance_email_subject: String,
    /// 余额不足提醒邮件 HTML 正文（支持 {{site_name}} {{balance}} {{threshold}}）
    #[serde(default = "default_low_balance_email_html")]
    pub low_balance_email_html: String,
}

fn default_true_notif() -> bool {
    true
}

fn default_low_balance_threshold() -> f64 {
    100.0
}

pub fn default_low_balance_email_subject() -> String {
    "【{{site_name}}】账户余额不足提醒".to_string()
}

pub fn default_low_balance_email_html() -> String {
    r#"<div style="font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif; max-width: 600px; margin: 0 auto; border: 1px solid #e8e8e8; border-radius: 8px;">
  <div style="padding: 30px;">
    <h2 style="color: #fa8c16; margin: 0 0 24px 0; font-size: 22px; font-weight: 600;">余额不足提醒</h2>
    <p style="color: #333; font-size: 16px; margin: 0 0 16px 0;">您好！</p>
    <p style="color: #333; font-size: 16px; margin: 0 0 24px 0;">您的账户可用余额已低于设定阈值，请及时充值以免影响服务使用。</p>
    <div style="background-color: #f5f5f5; padding: 20px; border-radius: 6px; margin-bottom: 24px;">
      <p style="color: #666; font-size: 14px; margin: 0 0 8px 0;">当前余额：<strong style="color: #fa541c; font-size: 18px;">{{balance}}</strong></p>
      <p style="color: #666; font-size: 14px; margin: 0;">提醒阈值：<strong>{{threshold}}</strong></p>
    </div>
    <div style="border-top: 1px dashed #e8e8e8; margin-top: 24px; padding-top: 16px;">
      <p style="color: #999; font-size: 12px; margin: 0;">此邮件由 {{site_name}} 系统根据您的通知订阅设置自动发送。</p>
    </div>
  </div>
</div>"#
    .to_string()
}

/// 渲染余额提醒模版变量
pub fn render_low_balance_template(
    template: &str,
    site_name: &str,
    balance: &str,
    threshold: &str,
) -> String {
    template
        .replace("{{site_name}}", site_name)
        .replace("{{balance}}", balance)
        .replace("{{threshold}}", threshold)
}

impl Default for NotificationSettings {
    fn default() -> Self {
        Self {
            site_notification_enabled: false,
            sms_balance_notification: false,
            email_balance_notification: false,
            web_notification_enabled: true,
            push_notification_enabled: true,
            do_not_disturb_enabled: true,
            low_balance_threshold: 100.0,
            low_balance_email_subject: default_low_balance_email_subject(),
            low_balance_email_html: default_low_balance_email_html(),
        }
    }
}

/// 聚合所有设置（读取）
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AllSettings {
    #[serde(default)]
    pub site: SiteSettings,
    #[serde(default)]
    pub currency: CurrencySettings,
    #[serde(default)]
    pub login: LoginSettings,
    #[serde(default)]
    pub registration: RegistrationSettings,
    #[serde(default)]
    pub smtp: SMTPSettings,
    #[serde(default)]
    pub sms: Option<SmsSettings>,
    #[serde(default)]
    pub marketing: MarketingSettings,
    #[serde(default)]
    pub database: DatabaseSettings,
    #[serde(default)]
    pub payment_wechat: Option<PaymentWechatSettings>,
    #[serde(default)]
    pub payment_alipay: Option<PaymentAlipaySettings>,
    #[serde(default)]
    pub payment_stripe: Option<PaymentStripeSettings>,
    #[serde(default)]
    pub payment_bonuspay: Option<PaymentBonuspaySettings>,
    #[serde(default)]
    pub payment_hyperbc: Option<PaymentHyperbcSettings>,
    #[serde(default)]
    pub payment_allinpay: Option<PaymentAllinpaySettings>,
    /// 支付渠道展示/排序/分渠道开关（与密钥配置解耦）
    #[serde(default)]
    pub payment_channels_ui: Option<PaymentChannelsUiSettings>,
    #[serde(default)]
    pub google_oauth: Option<GoogleOAuthSettings>,
    #[serde(default)]
    pub wechat_oauth: Option<WechatOAuthSettings>,
    #[serde(default)]
    pub agreement: AgreementSettings,
    #[serde(default)]
    pub storage: Option<StorageSettings>,
    #[serde(default)]
    pub menu_config: Option<MenuConfigSettings>,
    #[serde(default)]
    pub notification: NotificationSettings,
    #[serde(default)]
    pub relay: RelaySettings,
    #[serde(default, skip_deserializing)]
    pub server_timezone: Option<String>,
    #[serde(default, skip_deserializing)]
    pub server_time: Option<String>,
}

/// 更新设置请求（写入）
#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateSettingsRequest {
    #[serde(default)]
    pub site: Option<serde_json::Value>,
    #[serde(default)]
    pub currency: Option<serde_json::Value>,
    #[serde(default)]
    pub login: Option<serde_json::Value>,
    #[serde(default)]
    pub registration: Option<serde_json::Value>,
    #[serde(default)]
    pub smtp: Option<serde_json::Value>,
    #[serde(default)]
    pub sms: Option<serde_json::Value>,
    #[serde(default)]
    pub marketing: Option<serde_json::Value>,
    #[serde(default)]
    pub database: Option<serde_json::Value>,
    #[serde(default)]
    pub payment_wechat: Option<serde_json::Value>,
    #[serde(default)]
    pub payment_alipay: Option<serde_json::Value>,
    #[serde(default)]
    pub payment_stripe: Option<serde_json::Value>,
    #[serde(default)]
    pub payment_bonuspay: Option<serde_json::Value>,
    #[serde(default)]
    pub payment_hyperbc: Option<serde_json::Value>,
    #[serde(default)]
    pub payment_allinpay: Option<serde_json::Value>,
    #[serde(default)]
    pub payment_channels_ui: Option<serde_json::Value>,
    #[serde(default)]
    pub google_oauth: Option<serde_json::Value>,
    #[serde(default)]
    pub wechat_oauth: Option<serde_json::Value>,
    #[serde(default)]
    pub agreement: Option<serde_json::Value>,
    #[serde(default)]
    pub storage: Option<serde_json::Value>,
    #[serde(default)]
    pub menu_config: Option<serde_json::Value>,
    #[serde(default)]
    pub notification: Option<serde_json::Value>,
    #[serde(default)]
    pub relay: Option<serde_json::Value>,
}

// ════════════════════════════════════════════════════════════════════════════
// 【安全原则】公开接口返回的数据结构
//
// 以下 PublicSettings 系列结构体用于无需认证的公开接口返回值。
// 系统安全原则：隐私数据（密钥、密码、Secret、数据库信息等）绝不暴露到公开接口。
// 新增设置字段时，须评估是否属于公开数据。如为隐私数据，仅添加到 AllSettings，
// 不得添加到 PublicSettings。此原则必须被所有开发者（包括 AI）严格遵守。
// ════════════════════════════════════════════════════════════════════════════

/// 公开注册设置 — 仅暴露注册方式开关与绑定策略，隐藏 IP 限制、邮箱白名单等防刷细节
#[derive(Debug, Serialize, Clone)]
pub struct PublicRegistrationSettings {
    #[serde(default)]
    pub enable_username_registration: bool,
    #[serde(default)]
    pub enable_email_registration: bool,
    #[serde(default)]
    pub enable_mobile_registration: bool,
    #[serde(default)]
    pub enable_password_recovery: bool,
    #[serde(default)]
    pub require_bind_mobile: bool,
    #[serde(default)]
    pub require_bind_email: bool,
    #[serde(default = "default_bind_enforcement")]
    pub bind_enforcement: String,
    #[serde(default)]
    pub enable_user_kyc: bool,
}

impl From<&RegistrationSettings> for PublicRegistrationSettings {
    fn from(r: &RegistrationSettings) -> Self {
        Self {
            enable_username_registration: r.enable_username_registration,
            enable_email_registration: r.enable_email_registration,
            enable_mobile_registration: r.enable_mobile_registration,
            enable_password_recovery: r.enable_password_recovery,
            require_bind_mobile: r.require_bind_mobile,
            require_bind_email: r.require_bind_email,
            bind_enforcement: if r.bind_enforcement.is_empty() {
                default_bind_enforcement()
            } else {
                r.bind_enforcement.clone()
            },
            enable_user_kyc: r.enable_user_kyc,
        }
    }
}

/// 公开营销设置 — 仅暴露注册赠送 / 兑换开关，隐藏具体金额配置
#[derive(Debug, Serialize, Clone)]
pub struct PublicMarketingSettings {
    #[serde(default)]
    pub enable_registration_gift: bool,
    #[serde(default)]
    pub enable_redemption: bool,
}

impl From<&MarketingSettings> for PublicMarketingSettings {
    fn from(m: &MarketingSettings) -> Self {
        Self {
            enable_registration_gift: m.enable_registration_gift,
            enable_redemption: m.enable_redemption,
        }
    }
}

/// 公开支付状态 — 仅暴露各支付渠道的启用开关，不含任何密钥/密码/私钥
#[derive(Debug, Serialize, Clone, Default)]
pub struct PublicPaymentStatus {
    #[serde(default)]
    pub wechat_enabled: bool,
    #[serde(default)]
    pub alipay_enabled: bool,
    #[serde(default)]
    pub stripe_enabled: bool,
    #[serde(default)]
    pub bonuspay_enabled: bool,
    #[serde(default)]
    pub hyperbc_enabled: bool,
    #[serde(default)]
    pub allinpay_enabled: bool,
}

/// 公开通知设置
#[derive(Debug, Serialize, Clone)]
pub struct PublicNotificationSettings {
    #[serde(default)]
    pub site_notification_enabled: bool,
    #[serde(default)]
    pub sms_balance_notification: bool,
    #[serde(default)]
    pub email_balance_notification: bool,
    #[serde(default)]
    pub web_notification_enabled: bool,
    #[serde(default)]
    pub push_notification_enabled: bool,
    #[serde(default)]
    pub do_not_disturb_enabled: bool,
    #[serde(default)]
    pub low_balance_threshold: f64,
}

impl From<&NotificationSettings> for PublicNotificationSettings {
    fn from(n: &NotificationSettings) -> Self {
        Self {
            site_notification_enabled: n.site_notification_enabled,
            sms_balance_notification: n.sms_balance_notification,
            email_balance_notification: n.email_balance_notification,
            web_notification_enabled: n.web_notification_enabled,
            push_notification_enabled: n.push_notification_enabled,
            do_not_disturb_enabled: n.do_not_disturb_enabled,
            low_balance_threshold: if n.low_balance_threshold > 0.0 {
                n.low_balance_threshold
            } else {
                100.0
            },
        }
    }
}

/// 公开设置聚合 — 仅包含前端 UI 渲染所需的安全数据
///
/// 【安全】不包含任何密钥、密码、Secret、数据库、支付、SMTP、短信、存储等隐私配置。
/// OAuth 仅暴露 client_id / app_id（前端发起 OAuth 跳转必需），不暴露 secret。
#[derive(Debug, Serialize, Clone)]
pub struct PublicSettings {
    #[serde(default)]
    pub is_open_source: bool,
    #[serde(default)]
    pub site: SiteSettings,
    #[serde(default)]
    pub currency: CurrencySettings,
    #[serde(default)]
    pub login: LoginSettings,
    #[serde(default)]
    pub registration: PublicRegistrationSettings,
    #[serde(default)]
    pub marketing: PublicMarketingSettings,
    /// 各支付渠道启用状态（仅布尔值，不含密钥；兼容旧前端）
    #[serde(default)]
    pub payment: PublicPaymentStatus,
    /// 支付渠道列表（排序/展示名/副标题/logo/启用，不含密钥）
    #[serde(default)]
    pub payment_channels: Vec<PublicPaymentChannel>,
    #[serde(default)]
    pub agreement: AgreementSettings,
    /// 微信 OAuth app_id（前端扫码绑定/登录需要），不含 app_secret
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub wechat_oauth_app_id: Option<String>,
    /// Google OAuth client_id（前端 OAuth 跳转需要），不含 client_secret
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub google_oauth_client_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub menu_config: Option<MenuConfigSettings>,
    #[serde(default)]
    pub notification: PublicNotificationSettings,
}

#[cfg(test)]
mod payment_channel_i18n_tests {
    use super::*;

    #[test]
    fn public_channel_keeps_zh_and_en_names() {
        let ui = PaymentChannelsUiSettings {
            channels: vec![PaymentChannelUiItem {
                id: "alipay".into(),
                sort_order: 70,
                enabled: true,
                display_name: Some("  支付宝  ".into()),
                display_name_en: Some("  Alipay  ".into()),
                subtitle: Some("快捷".into()),
                subtitle_en: Some("Quick".into()),
                ..Default::default()
            }],
        };
        let gateway = PaymentGatewayEnableFlags {
            alipay: true,
            ..Default::default()
        };
        let list = build_public_payment_channels(&ui, &gateway);
        let alipay = list.iter().find(|c| c.id == "alipay").expect("alipay");
        assert_eq!(alipay.display_name.as_deref(), Some("支付宝"));
        assert_eq!(alipay.display_name_en.as_deref(), Some("Alipay"));
        assert_eq!(alipay.subtitle.as_deref(), Some("快捷"));
        assert_eq!(alipay.subtitle_en.as_deref(), Some("Quick"));
        assert!(alipay.enabled);
    }

    #[test]
    fn merge_preserves_english_display_fields() {
        let saved = PaymentChannelsUiSettings {
            channels: vec![PaymentChannelUiItem {
                id: "wechat".into(),
                sort_order: 88,
                enabled: true,
                display_name: Some("微信支付".into()),
                display_name_en: Some("WeChat Pay".into()),
                subtitle: Some("推荐".into()),
                subtitle_en: Some("Recommended".into()),
                ..Default::default()
            }],
        };
        let merged = merge_payment_channels_ui(Some(saved), &PaymentGatewayEnableFlags::default());
        let wechat = merged
            .channels
            .iter()
            .find(|c| c.id == "wechat")
            .expect("wechat");
        assert_eq!(wechat.display_name.as_deref(), Some("微信支付"));
        assert_eq!(wechat.display_name_en.as_deref(), Some("WeChat Pay"));
        assert_eq!(wechat.subtitle_en.as_deref(), Some("Recommended"));
        assert_eq!(wechat.sort_order, 88);
    }
}
