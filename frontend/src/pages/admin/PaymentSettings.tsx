/*
 * tokensbyte opensource
 * (c) 2026 tokensbyte.ai
 * @copyright      Copyright netbcloud/wstianxia
 * @license        MIT (https://www.tokensbyte.ai/)
 */

import React, { useEffect, useMemo, useState } from 'react';
import {
  Card, Form, Input, Button, message, Typography, Tabs, Switch, Alert, Divider,
  InputNumber, Table, Space, Image, Row, Col,
} from 'antd';
import {
  WechatOutlined, AlipayCircleOutlined, LinkOutlined, SafetyCertificateOutlined,
  DollarOutlined, CreditCardOutlined, ThunderboltOutlined, PlusOutlined, DeleteOutlined,
  SettingOutlined, BankOutlined, ArrowLeftOutlined, EditOutlined, CheckOutlined, CloseOutlined,
} from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import request from '../../utils/request';
import useSettingsStore from '../../store/settings';
import { useThemeStore } from '../../store/theme';
import {
  getChannelMeta,
  mergeChannelList,
  resolveChannelName,
  resolveChannelSubtitle,
  type PaymentChannelUiItem,
} from '../../constants/paymentChannels';

const { Title, Text } = Typography;

const PaymentSettings: React.FC = () => {
  const { t } = useTranslation();
  const { settings, updateStoreSettings } = useSettingsStore();
  const { themeMode } = useThemeStore();
  const isLight = themeMode === 'light';

  const [formCurrency] = Form.useForm();
  const [formDisplay] = Form.useForm();
  const [formGateway] = Form.useForm();

  const [loadingCurrency, setLoadingCurrency] = useState(false);
  const [loadingChannels, setLoadingChannels] = useState(false);
  const [savingDrawer, setSavingDrawer] = useState(false);
  const [channels, setChannels] = useState<PaymentChannelUiItem[]>([]);
  const [fullSettings, setFullSettings] = useState<any>(null);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [activeTab, setActiveTab] = useState('currency');
  const [editingSubtitleField, setEditingSubtitleField] = useState<string | null>(null);
  const [editingSubtitleVal, setEditingSubtitleVal] = useState<string>('');

  const siteOrigin = window.location.origin;
  const notifyUrl = (path: string) => `${siteOrigin}/api/v1/finance/pay/notify/${path}`;

  useEffect(() => { fetchSettings(); }, []);

  const fetchSettings = async () => {
    setLoadingChannels(true);
    try {
      const response = await (request.get('/settings/full') as any);
      setFullSettings(response);
      if (response?.currency) {
        const amountsStr = (response.currency.quick_amounts || [20, 50, 100, 500, 1000, 5000]).join(', ');
        formCurrency.setFieldsValue({
          ...response.currency,
          quick_amounts: amountsStr,
          min_recharge_amount: response.currency.min_recharge_amount ?? 5,
        });
      }
      setChannels(mergeChannelList(response?.payment_channels_ui?.channels));
    } catch (error) {
      console.error('Failed to fetch payment settings:', error);
    } finally {
      setLoadingChannels(false);
    }
  };

  const persistChannels = async (
    nextChannels: PaymentChannelUiItem[],
    extraPayload: Record<string, any> = {},
  ) => {
    const payload = {
      payment_channels_ui: { channels: nextChannels },
      ...extraPayload,
    };
    const updatedSettings = await (request.post('/settings', payload) as any);
    updateStoreSettings(updatedSettings);
    setFullSettings((prev: any) => ({
      ...(prev || {}),
      ...extraPayload,
      payment_channels_ui: { channels: nextChannels },
      ...(updatedSettings || {}),
    }));
    setChannels(mergeChannelList(nextChannels));
    return updatedSettings;
  };

  const syncGatewayEnabled = (channelId: string, enabled: boolean, _nextChannels: PaymentChannelUiItem[]) => {
    const meta = getChannelMeta(channelId);
    if (!meta) return {};
    return {
      [meta.gatewayKey]: {
        ...(fullSettings?.[meta.gatewayKey] || {}),
        enabled,
      },
    };
  };

  const onToggleChannel = async (id: string, enabled: boolean) => {
    const next = channels.map((c) => {
      if (c.id !== id) return c;
      const updated: PaymentChannelUiItem = { ...c, enabled };
      if (id === 'allinpay' && enabled && !c.allinpay_wechat_enabled && !c.allinpay_alipay_enabled) {
        updated.allinpay_wechat_enabled = true;
        updated.allinpay_alipay_enabled = true;
      }
      return updated;
    });
    try {
      await persistChannels(next, syncGatewayEnabled(id, enabled, next));
      message.destroy();
      message.success(enabled ? '已开启' : '已关闭');
    } catch (e) {
      console.error(e);
      message.error(t('common.error'));
      await fetchSettings();
    }
  };

  const onSortChange = async (id: string, sort_order: number) => {
    const next = channels.map((c) => (c.id === id ? { ...c, sort_order } : c));
    setChannels(mergeChannelList(next));
    try {
      await persistChannels(next);
    } catch (e) {
      console.error(e);
      message.error(t('common.error'));
      await fetchSettings();
    }
  };

  const onFieldCommit = async (
    id: string,
    field: 'display_name' | 'display_name_en' | 'subtitle' | 'subtitle_en',
    raw: string,
  ) => {
    const meta = getChannelMeta(id);
    const defaultVal =
      field === 'display_name'
        ? meta?.defaultName || ''
        : field === 'display_name_en'
        ? meta?.defaultNameEn || ''
        : field === 'subtitle'
        ? meta?.defaultSubtitle || ''
        : meta?.defaultSubtitleEn || '';

    const trimmed = raw.trim();
    const val = !trimmed || trimmed === defaultVal ? null : trimmed;
    const currentItem = channels.find((c) => c.id === id);
    const prev = (currentItem?.[field] || '').trim() || null;
    const prevNorm = !prev || prev === defaultVal ? null : prev;

    if (prevNorm === val) return;

    const next = channels.map((c) => (c.id === id ? { ...c, [field]: val } : c));
    setChannels(mergeChannelList(next));
    try {
      await persistChannels(next);
    } catch (e) {
      console.error(e);
      message.error(t('common.error'));
      await fetchSettings();
    }
  };

  const openConfig = (id: string) => {
    const item = channels.find((c) => c.id === id);
    const meta = getChannelMeta(id);
    if (!item || !meta) return;
    setEditingId(id);
    formDisplay.setFieldsValue({
      enabled: item.enabled,
      sort_order: item.sort_order,
      subtitle: item.subtitle || '',
      subtitle_en: item.subtitle_en || '',
      logo_url: item.logo_url || '',
      allinpay_wechat_enabled: item.allinpay_wechat_enabled !== false,
      allinpay_alipay_enabled: item.allinpay_alipay_enabled !== false,
    });
    const gw = fullSettings?.[meta.gatewayKey] || {};
    formGateway.setFieldsValue({ ...gw });
  };

  const closeEditor = () => {
    setEditingId(null);
    setActiveTab('channels');
    formDisplay.resetFields();
    formGateway.resetFields();
  };

  const onSaveDrawer = async () => {
    if (!editingId) return;
    const meta = getChannelMeta(editingId);
    if (!meta) return;
    try {
      const displayValues = await formDisplay.validateFields();
      const gatewayValues = await formGateway.validateFields();

      if (editingId === 'allinpay') {
        const wechatOn = !!displayValues.allinpay_wechat_enabled;
        const alipayOn = !!displayValues.allinpay_alipay_enabled;
        if (!wechatOn && !alipayOn) {
          message.error('请至少开启一个通联子渠道（微信或支付宝）');
          return;
        }
      }

      setSavingDrawer(true);

      const nextChannels = channels.map((c) => {
        if (c.id !== editingId) return c;
        const updated: PaymentChannelUiItem = {
          ...c,
          enabled: !!displayValues.enabled,
          sort_order: Number(displayValues.sort_order) || 0,
          subtitle: (displayValues.subtitle || '').trim() || null,
          subtitle_en: (displayValues.subtitle_en || '').trim() || null,
          logo_url: (displayValues.logo_url || '').trim() || null,
        };
        if (editingId === 'allinpay') {
          updated.allinpay_wechat_enabled = !!displayValues.allinpay_wechat_enabled;
          updated.allinpay_alipay_enabled = !!displayValues.allinpay_alipay_enabled;
        }
        return updated;
      });

      let gatewayPayload: Record<string, any> = {};
      if (meta.gatewayKey === 'payment_wechat') {
        gatewayPayload = {
          payment_wechat: {
            ...(fullSettings?.payment_wechat || {}),
            enabled: !!displayValues.enabled,
            mchid: gatewayValues.mchid || '',
            appid: gatewayValues.appid || '',
            api_v3_key: gatewayValues.api_v3_key || '',
            cert_serial_no: gatewayValues.cert_serial_no || '',
            private_key: gatewayValues.private_key || '',
          },
        };
      } else if (meta.gatewayKey === 'payment_alipay') {
        gatewayPayload = {
          payment_alipay: {
            ...(fullSettings?.payment_alipay || {}),
            enabled: !!displayValues.enabled,
            app_id: gatewayValues.app_id || '',
            private_key: gatewayValues.private_key || '',
            alipay_public_key: gatewayValues.alipay_public_key || '',
            sign_type: 'RSA2',
          },
        };
      } else if (meta.gatewayKey === 'payment_stripe') {
        gatewayPayload = {
          payment_stripe: {
            ...(fullSettings?.payment_stripe || {}),
            enabled: !!displayValues.enabled,
            secret_key: gatewayValues.secret_key || '',
            publishable_key: gatewayValues.publishable_key || '',
            webhook_secret: gatewayValues.webhook_secret || '',
          },
        };
      } else if (meta.gatewayKey === 'payment_bonuspay') {
        gatewayPayload = {
          payment_bonuspay: {
            ...(fullSettings?.payment_bonuspay || {}),
            enabled: !!displayValues.enabled,
            partner_id: gatewayValues.partner_id || '',
            merchant_private_key: gatewayValues.merchant_private_key || '',
            bonuspay_public_key: gatewayValues.bonuspay_public_key || '',
            api_url: gatewayValues.api_url || 'https://api.bonuspay.network',
            crypto_exchange_rate: gatewayValues.crypto_exchange_rate || 1.0,
          },
        };
      } else if (meta.gatewayKey === 'payment_hyperbc') {
        gatewayPayload = {
          payment_hyperbc: {
            ...(fullSettings?.payment_hyperbc || {}),
            enabled: !!displayValues.enabled,
            app_id: gatewayValues.app_id || '',
            merchant_private_key: gatewayValues.merchant_private_key || '',
            hyperbc_public_key: gatewayValues.hyperbc_public_key || '',
            api_url: gatewayValues.api_url || 'https://api.cipherbc.com/shopapi',
            crypto_exchange_rate: gatewayValues.crypto_exchange_rate || 1.0,
          },
        };
      } else if (meta.gatewayKey === 'payment_allinpay') {
        gatewayPayload = {
          payment_allinpay: {
            enabled: !!displayValues.enabled,
            cusid: gatewayValues.cusid || '',
            appid: gatewayValues.appid || '',
            merchant_private_key: gatewayValues.merchant_private_key || '',
            allinpay_public_key: gatewayValues.allinpay_public_key || '',
            sign_type: 'RSA',
            api_url: gatewayValues.api_url || 'https://vsp.allinpay.com/apiweb',
            version: gatewayValues.version || '11',
          },
        };
      }

      await persistChannels(nextChannels, gatewayPayload);
      message.destroy();
      message.success('支付渠道配置已保存');
      closeEditor();
      await fetchSettings();
    } catch (error: any) {
      if (error?.errorFields) return;
      console.error('Save channel error:', error);
      message.error(t('common.error'));
    } finally {
      setSavingDrawer(false);
    }
  };

  const onFinishCurrency = async (values: any) => {
    setLoadingCurrency(true);
    try {
      const quick_amounts = (String(values.quick_amounts || ''))
        .split(',')
        .map((x: string) => parseFloat(x.trim()))
        .filter((x: number) => !isNaN(x) && x > 0);

      const payload = {
        currency: {
          ...settings?.currency,
          default_currency: values.default_currency,
          currency_symbol: values.currency_symbol,
          currency_unit: values.currency_unit,
          token_ratio: values.token_ratio,
          auxiliary_currencies: values.auxiliary_currencies || [],
          quick_amounts,
          min_recharge_amount: values.min_recharge_amount != null ? parseFloat(values.min_recharge_amount) : 5.0,
        },
      };
      const updatedSettings = await (request.post('/settings', payload) as any);
      message.destroy();
      message.success(t('settings.save_success', '货币设置保存成功'));
      updateStoreSettings(updatedSettings);
    } catch (error) {
      console.error('Save currency error:', error);
      message.error(t('common.error'));
    } finally {
      setLoadingCurrency(false);
    }
  };

  const notifyUrlBlock = (url: string, label: string) => (
    <div style={{
      display: 'flex', alignItems: 'center', gap: 8,
      background: 'rgba(22, 119, 255, 0.06)',
      border: '1px dashed rgba(22, 119, 255, 0.3)',
      borderRadius: 8, padding: '10px 14px', marginBottom: 8,
    }}>
      <LinkOutlined style={{ color: '#1677ff', flexShrink: 0 }} />
      <div style={{ flex: 1, minWidth: 0 }}>
        <Text type="secondary" style={{ fontSize: 11, display: 'block' }}>{label}</Text>
        <Text copyable={{ text: url }} style={{ fontFamily: 'monospace', fontSize: 12, wordBreak: 'break-all' }}>{url}</Text>
      </div>
    </div>
  );

  const channelIcon = (id: string, logoUrl?: string | null, size = 22) => {
    const url = (logoUrl || '').trim();
    if (url) {
      return <Image src={url} width={size} height={size} preview={false} style={{ objectFit: 'contain', borderRadius: 4 }} />;
    }
    const meta = getChannelMeta(id);
    const color = meta?.accent || '#666';
    if (id === 'alipay') return <AlipayCircleOutlined style={{ fontSize: size, color }} />;
    if (id === 'wechat') return <WechatOutlined style={{ fontSize: size, color }} />;
    if (id === 'allinpay') return <ThunderboltOutlined style={{ fontSize: size, color }} />;
    if (id === 'stripe') return <CreditCardOutlined style={{ fontSize: size, color }} />;
    if (id === 'bonuspay') return <ThunderboltOutlined style={{ fontSize: size, color }} />;
    if (id === 'hyperbc') return <span style={{ fontSize: size - 2, fontWeight: 'bold', color }}>₿</span>;
    return <BankOutlined style={{ fontSize: size, color }} />;
  };

  const sortedChannels = useMemo(
    () => [...channels].sort((a, b) => (b.sort_order || 0) - (a.sort_order || 0) || a.id.localeCompare(b.id)),
    [channels],
  );

  const editingMeta = editingId ? getChannelMeta(editingId) : undefined;

  const renderGatewayFields = () => {
    if (!editingMeta) return null;
    const key = editingMeta.gatewayKey;

    if (key === 'payment_wechat') {
      return (
        <>
          <Alert type="info" showIcon icon={<SafetyCertificateOutlined />} style={{ marginBottom: 16, borderRadius: 8 }}
            title="微信支付 API v3 接入指引"
            description={<div style={{ fontSize: 13, lineHeight: 1.8 }}>
              <div>1. 登录 <a href="https://pay.weixin.qq.com" target="_blank" rel="noreferrer">微信支付商户平台</a> 获取商户号 / API v3 密钥 / 证书</div>
              <div>2. 开通 Native 支付，并绑定公众号/小程序 AppID</div>
            </div>}
          />
          {notifyUrlBlock(notifyUrl('wechat'), '微信支付异步回调通知地址')}
          <Form.Item label="商户号 (MCHID)" name="mchid" rules={[{ required: true, message: '请输入微信支付商户号' }]}>
            <Input placeholder="例如：1900000109" />
          </Form.Item>
          <Form.Item label="应用 AppID" name="appid" rules={[{ required: true, message: '请输入绑定的 AppID' }]}>
            <Input placeholder="例如：wx8888888888888888" />
          </Form.Item>
          <Form.Item label="API v3 密钥" name="api_v3_key" rules={[{ required: true, message: '请输入 API v3 密钥' }]}>
            <Input.Password placeholder="32位字符串密钥" />
          </Form.Item>
          <Form.Item label="商户证书序列号" name="cert_serial_no" rules={[{ required: true, message: '请输入商户证书序列号' }]}>
            <Input placeholder="例如：7F5C2B3A..." />
          </Form.Item>
          <Form.Item label="商户私钥 (apiclient_key.pem)" name="private_key" rules={[{ required: true, message: '请粘贴私钥全部内容' }]}>
            <Input.TextArea rows={5} placeholder="-----BEGIN PRIVATE KEY-----" style={{ fontFamily: 'monospace', fontSize: 12 }} />
          </Form.Item>
        </>
      );
    }

    if (key === 'payment_alipay') {
      return (
        <>
          <Alert type="info" showIcon style={{ marginBottom: 16, borderRadius: 8 }}
            title="支付宝电脑网站支付接入指引"
            description={<div style={{ fontSize: 13, lineHeight: 1.8 }}>
              <div>1. 登录 <a href="https://open.alipay.com" target="_blank" rel="noreferrer">支付宝开放平台</a> 创建应用并获取 App ID</div>
              <div>2. 配置 RSA2 密钥，签约电脑网站支付</div>
            </div>}
          />
          {notifyUrlBlock(notifyUrl('alipay'), '支付宝异步回调通知地址')}
          <Form.Item label="App ID" name="app_id" rules={[{ required: true, message: '请输入支付宝应用 AppID' }]}>
            <Input placeholder="例如：2021000000000000" />
          </Form.Item>
          <Form.Item label="应用私钥" name="private_key" rules={[{ required: true, message: '请输入应用私钥' }]}>
            <Input.TextArea rows={5} placeholder="粘贴 RSA2 应用私钥" style={{ fontFamily: 'monospace', fontSize: 12 }} />
          </Form.Item>
          <Form.Item label="支付宝公钥" name="alipay_public_key" rules={[{ required: true, message: '请输入支付宝公钥' }]}>
            <Input.TextArea rows={4} placeholder="粘贴支付宝公钥" style={{ fontFamily: 'monospace', fontSize: 12 }} />
          </Form.Item>
        </>
      );
    }

    if (key === 'payment_stripe') {
      return (
        <>
          <Alert type="info" showIcon style={{ marginBottom: 16, borderRadius: 8 }}
            title="Stripe Checkout 接入指引"
            description={<div style={{ fontSize: 13, lineHeight: 1.8 }}>
              <div>1. 在 Stripe Dashboard 获取 Secret Key / Publishable Key</div>
              <div>2. Webhooks 监听 checkout.session.completed，填入下方回调地址</div>
            </div>}
          />
          {notifyUrlBlock(notifyUrl('stripe'), 'Stripe Webhook 回调地址')}
          <Form.Item label="Secret Key" name="secret_key" rules={[{ required: true, message: '请输入 Stripe Secret Key' }]}>
            <Input.Password placeholder="sk_live_xxxx 或 sk_test_xxxx" />
          </Form.Item>
          <Form.Item label="Publishable Key" name="publishable_key" rules={[{ required: true, message: '请输入 Stripe Publishable Key' }]}>
            <Input placeholder="pk_live_xxxx 或 pk_test_xxxx" />
          </Form.Item>
          <Form.Item label="Webhook Signing Secret" name="webhook_secret" rules={[{ required: true, message: '请输入 Webhook Secret' }]}>
            <Input.Password placeholder="whsec_xxxx" />
          </Form.Item>
        </>
      );
    }

    if (key === 'payment_bonuspay') {
      return (
        <>
          <Alert type="info" showIcon style={{ marginBottom: 16, borderRadius: 8 }}
            title="BonusPay 接入指引"
            description={<div style={{ fontSize: 13, lineHeight: 1.8 }}>
              <div>在 bonuspay.network 获取 Partner-Id，并配置 RSA 密钥与回调地址</div>
            </div>}
          />
          {notifyUrlBlock(notifyUrl('bonuspay'), 'BonusPay 异步回调通知地址')}
          <Form.Item label="Partner-Id" name="partner_id" rules={[{ required: true, message: '请输入 Partner-Id' }]}>
            <Input placeholder="例如：200000000888" />
          </Form.Item>
          <Form.Item label="商户 RSA 私钥" name="merchant_private_key" rules={[{ required: true, message: '请输入商户私钥' }]}>
            <Input.TextArea rows={4} style={{ fontFamily: 'monospace', fontSize: 12 }} />
          </Form.Item>
          <Form.Item label="BonusPay RSA 公钥" name="bonuspay_public_key" rules={[{ required: true, message: '请输入 BonusPay 公钥' }]}>
            <Input.TextArea rows={4} style={{ fontFamily: 'monospace', fontSize: 12 }} />
          </Form.Item>
          <Form.Item label="API 接口地址" name="api_url">
            <Input placeholder="https://api.bonuspay.network" />
          </Form.Item>
          <Form.Item label="USDT / USDC 汇率" name="crypto_exchange_rate" rules={[{ required: true }]}>
            <InputNumber min={0.01} step={0.1} style={{ width: '100%' }} />
          </Form.Item>
        </>
      );
    }

    if (key === 'payment_hyperbc') {
      return (
        <>
          <Alert type="info" showIcon style={{ marginBottom: 16, borderRadius: 8 }}
            title="HyperBC 接入指引"
            description={<div style={{ fontSize: 13, lineHeight: 1.8 }}>在 HyperBC 商户后台获取 APP_ID 与密钥，并配置回调地址</div>}
          />
          {notifyUrlBlock(notifyUrl('hyperbc'), 'HyperBC 异步回调通知地址')}
          <Form.Item label="APP_ID" name="app_id" rules={[{ required: true, message: '请输入 APP_ID' }]}>
            <Input />
          </Form.Item>
          <Form.Item label="商户 RSA 私钥" name="merchant_private_key" rules={[{ required: true, message: '请输入商户私钥' }]}>
            <Input.TextArea rows={4} style={{ fontFamily: 'monospace', fontSize: 12 }} />
          </Form.Item>
          <Form.Item label="HyperBC 平台公钥" name="hyperbc_public_key" rules={[{ required: true, message: '请输入平台公钥' }]}>
            <Input.TextArea rows={4} style={{ fontFamily: 'monospace', fontSize: 12 }} />
          </Form.Item>
          <Form.Item label="API 接口地址" name="api_url">
            <Input placeholder="https://api.cipherbc.com/shopapi" />
          </Form.Item>
          <Form.Item label="加密货币汇率" name="crypto_exchange_rate" rules={[{ required: true }]}>
            <InputNumber min={0.01} step={0.1} style={{ width: '100%' }} />
          </Form.Item>
        </>
      );
    }

    if (key === 'payment_allinpay') {
      return (
        <>
          <Alert type="info" showIcon style={{ marginBottom: 16, borderRadius: 8 }}
            title="通联收银宝接入指引"
            description={<div style={{ fontSize: 13, lineHeight: 1.8 }}>
              <div>通联支付为单一通道，可在上方分别开启微信 / 支付宝子渠道；用户端默认看到「通联支付」。</div>
            </div>}
          />
          {notifyUrlBlock(notifyUrl('allinpay'), '通联支付异步回调通知地址')}
          <Form.Item label="商户号 (cusid)" name="cusid" rules={[{ required: true, message: '请输入通联商户号' }]}>
            <Input />
          </Form.Item>
          <Form.Item label="应用ID (appid)" name="appid" rules={[{ required: true, message: '请输入通联应用ID' }]}>
            <Input />
          </Form.Item>
          <Form.Item label="商户 RSA 私钥" name="merchant_private_key" rules={[{ required: true, message: '请输入商户私钥' }]}>
            <Input.TextArea rows={4} style={{ fontFamily: 'monospace', fontSize: 12 }} />
          </Form.Item>
          <Form.Item label="通联 RSA 公钥" name="allinpay_public_key" rules={[{ required: true, message: '请输入通联平台公钥' }]}>
            <Input.TextArea rows={4} style={{ fontFamily: 'monospace', fontSize: 12 }} />
          </Form.Item>
          <Form.Item label="API 接口网关地址" name="api_url" rules={[{ required: true }]} initialValue="https://vsp.allinpay.com/apiweb">
            <Input />
          </Form.Item>
          <Form.Item label="协议版本号" name="version" rules={[{ required: true }]} initialValue="11">
            <Input />
          </Form.Item>
        </>
      );
    }

    return null;
  };

  const channelColumns = [
    {
      title: 'Logo',
      key: 'logo',
      width: 68,
      align: 'center' as const,
      render: (_: unknown, record: PaymentChannelUiItem) => (
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
          {channelIcon(record.id, record.logo_url, 26)}
        </div>
      ),
    },
    {
      title: '中文配置',
      key: 'zh_config',
      render: (_: unknown, record: PaymentChannelUiItem) => {
        const meta = getChannelMeta(record.id);
        const nameZh = resolveChannelName(record, 'zh');
        const subHint = record.id === 'allinpay'
          ? [
              record.allinpay_wechat_enabled !== false ? '微信' : null,
              record.allinpay_alipay_enabled !== false ? '支付宝' : null,
            ].filter(Boolean).join(' / ') || '未开子渠道'
          : '';
        const isEditing = editingSubtitleField === `${record.id}:subtitle`;
        const subZhDisplay = record.subtitle || meta?.defaultSubtitle || '无';

        return (
          <div style={{ display: 'flex', flexDirection: 'column', gap: 4, minWidth: 180, maxWidth: 300 }}>
            <div style={{ fontWeight: 600, fontSize: 14 }}>{nameZh}</div>
            {isEditing ? (
              <div style={{ display: 'flex', alignItems: 'center', gap: 4, marginTop: 2 }}>
                <Input
                  size="small"
                  autoFocus
                  value={editingSubtitleVal}
                  placeholder={meta?.defaultSubtitle || '无'}
                  maxLength={32}
                  allowClear
                  onChange={(e) => setEditingSubtitleVal(e.target.value)}
                  onPressEnter={() => {
                    onFieldCommit(record.id, 'subtitle', editingSubtitleVal);
                    setEditingSubtitleField(null);
                  }}
                  onKeyDown={(e) => {
                    if (e.key === 'Escape') setEditingSubtitleField(null);
                  }}
                  style={{ width: 140 }}
                />
                <Button
                  size="small"
                  type="text"
                  icon={<CheckOutlined style={{ color: '#52c41a' }} />}
                  onClick={() => {
                    onFieldCommit(record.id, 'subtitle', editingSubtitleVal);
                    setEditingSubtitleField(null);
                  }}
                  title="保存"
                />
                <Button
                  size="small"
                  type="text"
                  icon={<CloseOutlined style={{ color: '#ff4d4f' }} />}
                  onClick={() => setEditingSubtitleField(null)}
                  title="取消"
                />
              </div>
            ) : (
              <div style={{ display: 'flex', alignItems: 'center', gap: 6, marginTop: 2 }}>
                <Text
                  type="secondary"
                  style={{
                    fontSize: 13,
                    maxWidth: 200,
                    overflow: 'hidden',
                    textOverflow: 'ellipsis',
                    whiteSpace: 'nowrap',
                  }}
                >
                  {subZhDisplay}
                </Text>
                <Button
                  type="text"
                  size="small"
                  icon={<EditOutlined style={{ fontSize: 13, color: '#1677ff' }} />}
                  onClick={() => {
                    setEditingSubtitleField(`${record.id}:subtitle`);
                    setEditingSubtitleVal(record.subtitle ?? meta?.defaultSubtitle ?? '');
                  }}
                  title="编辑副标题"
                  style={{ padding: '0 2px', height: 20 }}
                />
              </div>
            )}
            {subHint ? (
              <Text type="secondary" style={{ fontSize: 11, display: 'block', marginTop: 2 }}>
                子渠道：{subHint}
              </Text>
            ) : null}
          </div>
        );
      },
    },
    {
      title: '英文配置',
      key: 'en_config',
      render: (_: unknown, record: PaymentChannelUiItem) => {
        const meta = getChannelMeta(record.id);
        const nameEn = resolveChannelName(record, 'en');
        const isEditing = editingSubtitleField === `${record.id}:subtitle_en`;
        const subEnDisplay = record.subtitle_en || meta?.defaultSubtitleEn || 'None';

        return (
          <div style={{ display: 'flex', flexDirection: 'column', gap: 4, minWidth: 180, maxWidth: 300 }}>
            <div style={{ fontWeight: 600, fontSize: 14 }}>{nameEn}</div>
            {isEditing ? (
              <div style={{ display: 'flex', alignItems: 'center', gap: 4, marginTop: 2 }}>
                <Input
                  size="small"
                  autoFocus
                  value={editingSubtitleVal}
                  placeholder={meta?.defaultSubtitleEn || 'None'}
                  maxLength={32}
                  allowClear
                  onChange={(e) => setEditingSubtitleVal(e.target.value)}
                  onPressEnter={() => {
                    onFieldCommit(record.id, 'subtitle_en', editingSubtitleVal);
                    setEditingSubtitleField(null);
                  }}
                  onKeyDown={(e) => {
                    if (e.key === 'Escape') setEditingSubtitleField(null);
                  }}
                  style={{ width: 140 }}
                />
                <Button
                  size="small"
                  type="text"
                  icon={<CheckOutlined style={{ color: '#52c41a' }} />}
                  onClick={() => {
                    onFieldCommit(record.id, 'subtitle_en', editingSubtitleVal);
                    setEditingSubtitleField(null);
                  }}
                  title="Save"
                />
                <Button
                  size="small"
                  type="text"
                  icon={<CloseOutlined style={{ color: '#ff4d4f' }} />}
                  onClick={() => setEditingSubtitleField(null)}
                  title="Cancel"
                />
              </div>
            ) : (
              <div style={{ display: 'flex', alignItems: 'center', gap: 6, marginTop: 2 }}>
                <Text
                  type="secondary"
                  style={{
                    fontSize: 13,
                    maxWidth: 200,
                    overflow: 'hidden',
                    textOverflow: 'ellipsis',
                    whiteSpace: 'nowrap',
                  }}
                >
                  {subEnDisplay}
                </Text>
                <Button
                  type="text"
                  size="small"
                  icon={<EditOutlined style={{ fontSize: 13, color: '#1677ff' }} />}
                  onClick={() => {
                    setEditingSubtitleField(`${record.id}:subtitle_en`);
                    setEditingSubtitleVal(record.subtitle_en ?? meta?.defaultSubtitleEn ?? '');
                  }}
                  title="Edit English subtitle"
                  style={{ padding: '0 2px', height: 20 }}
                />
              </div>
            )}
          </div>
        );
      },
    },
    {
      title: '排序',
      key: 'sort_order',
      width: 110,
      sorter: (a: PaymentChannelUiItem, b: PaymentChannelUiItem) => (a.sort_order || 0) - (b.sort_order || 0),
      defaultSortOrder: 'descend' as const,
      render: (_: unknown, record: PaymentChannelUiItem) => (
        <InputNumber
          size="small"
          min={0}
          max={9999}
          value={record.sort_order || 0}
          onChange={(val) => onSortChange(record.id, val ?? 0)}
          style={{ width: 88 }}
        />
      ),
    },
    {
      title: '状态',
      key: 'enabled',
      width: 90,
      render: (_: unknown, record: PaymentChannelUiItem) => (
        <Switch checked={!!record.enabled} onChange={(v) => onToggleChannel(record.id, v)} />
      ),
    },
    {
      title: '操作',
      key: 'action',
      width: 90,
      render: (_: unknown, record: PaymentChannelUiItem) => (
        <Button type="link" icon={<SettingOutlined />} onClick={() => openConfig(record.id)}>
          配置
        </Button>
      ),
    },
  ];

  const tabItems = [
    {
      key: 'currency',
      label: <span><DollarOutlined style={{ color: '#faad14' }} /> {t('menu.currency_settings', '货币设置')}</span>,
      children: (
        <div style={{ maxWidth: 640, marginTop: 16 }}>
          <Form form={formCurrency} layout="vertical" onFinish={onFinishCurrency} autoComplete="off">
            <Form.Item label={t('settings.default_currency', '默认货币代码')} name="default_currency" rules={[{ required: true }]}><Input placeholder="CNY" /></Form.Item>
            <Form.Item label={t('settings.currency_symbol', '货币符号')} name="currency_symbol" rules={[{ required: true }]}><Input placeholder="¥" /></Form.Item>
            <Form.Item label={t('settings.currency_unit', '货币单位')} name="currency_unit" rules={[{ required: true }]}><Input placeholder="元" /></Form.Item>
            <Form.Item noStyle dependencies={['default_currency', 'token_ratio']}>
              {({ getFieldValue }) => {
                const c = getFieldValue('default_currency') || 'USD';
                const ratio = getFieldValue('token_ratio');
                const ratioStr = (ratio !== undefined && ratio !== null) ? ratio : 'N';
                return (
                  <Form.Item label={t('settings.token_ratio', '兑换比例')} name="token_ratio" rules={[{ required: true }]} extra={<Text type="secondary">{`1 ${c} = ${ratioStr} Tokens`}</Text>}>
                    <InputNumber style={{ width: '100%' }} min={0} step={0.0001} />
                  </Form.Item>
                );
              }}
            </Form.Item>

            <Divider>辅助货币显示设置</Divider>
            <Form.List name="auxiliary_currencies">
              {(fields, { add, remove }) => (
                <>
                  {fields.map(({ key, name, ...restField }) => (
                    <div key={key} style={{ display: 'flex', gap: 8, alignItems: 'center', marginBottom: 16 }}>
                      <Form.Item {...restField} name={[name, 'code']} rules={[{ required: true, message: '代码' }]} style={{ margin: 0, flex: 1 }}>
                        <Input placeholder="货币代码 (如 USD)" />
                      </Form.Item>
                      <Form.Item {...restField} name={[name, 'symbol']} rules={[{ required: true, message: '符号' }]} style={{ margin: 0, flex: 1 }}>
                        <Input placeholder="货币符号 (如 $)" />
                      </Form.Item>
                      <Form.Item {...restField} name={[name, 'exchange_rate']} rules={[{ required: true, message: '汇率' }]} style={{ margin: 0, flex: 1 }}>
                        <InputNumber style={{ width: '100%' }} min={0.0001} step={0.0001} placeholder="1主货币=?此货币汇率" />
                      </Form.Item>
                      <Form.Item {...restField} name={[name, 'enabled']} valuePropName="checked" style={{ margin: 0 }}>
                        <Switch />
                      </Form.Item>
                      <Button danger onClick={() => remove(name)} type="text" icon={<DeleteOutlined />} />
                    </div>
                  ))}
                  <Form.Item>
                    <Button type="dashed" onClick={() => add({ enabled: true, exchange_rate: 1.0 })} block icon={<PlusOutlined />}>
                      添加辅助货币
                    </Button>
                    <div style={{ marginTop: 8, fontSize: 12, color: isLight ? 'rgba(0, 0, 0, 0.45)' : 'rgba(255, 255, 255, 0.45)' }}>
                      设置后，在模型广场和后台模型列表中可切换显示不同货币价格作为参考。<br />
                      所有的计价都是以站点默认货币为基准（默认货币的基准就是 1，不需要再填写添加）。<br />
                      汇率说明：填写 1 主货币(如 CNY) 对应的此货币(如 USD) 数量，比如 1 CNY = 0.14 USD，则填写 0.14。
                    </div>
                  </Form.Item>
                </>
              )}
            </Form.List>

            <Divider>通用充值设置</Divider>
            <Form.Item
              label="快捷支付金额"
              name="quick_amounts"
              rules={[{ required: true, message: '请输入快捷支付金额' }]}
              extra="多个金额请用英文逗号分隔，例如：20, 50, 100, 500, 1000, 5000"
            >
              <Input placeholder="20, 50, 100, 500, 1000, 5000" />
            </Form.Item>

            <Form.Item
              label="最小充值金额限制"
              name="min_recharge_amount"
              rules={[{ required: true, message: '请输入最小充值金额' }]}
              extra="设置用户单次最小的充值金额。设置为 0 代表无限制，默认值为 5"
            >
              <InputNumber style={{ width: '100%' }} min={0} step={1} />
            </Form.Item>

            <Form.Item>
              <Button type="primary" htmlType="submit" loading={loadingCurrency} size="large" style={{ borderRadius: 8 }}>
                {t('common.save', '保存设置')}
              </Button>
            </Form.Item>
          </Form>
        </div>
      ),
    },
    {
      key: 'channels',
      label: <span><BankOutlined style={{ color: '#1677ff' }} /> 支付渠道</span>,
      children: (
        <div style={{ marginTop: 16 }}>
          <Alert
            type="info"
            showIcon
            style={{ marginBottom: 16, borderRadius: 8 }}
            title="支付渠道列表"
            description="排序数字越大越靠前。中文与英文副标题默认直接展示，点击后方编辑按钮可进行修改，留空则使用系统默认。点击「配置」可修改网关参数、Logo及子渠道。用户端中文站点显示中文配置，其他语言显示英文配置。"
          />
          <Table
            rowKey="id"
            loading={loadingChannels}
            dataSource={sortedChannels}
            columns={channelColumns}
            pagination={false}
            size="middle"
          />
        </div>
      ),
    },
  ];

  return (
    <Card bordered={false} style={{ borderRadius: 12 }}>
      {!editingId ? (
        <>
          <div style={{ display: 'flex', alignItems: 'center', gap: 12, marginBottom: 20 }}>
            <SafetyCertificateOutlined style={{ fontSize: 24, color: '#52c41a' }} />
            <Title level={3} style={{ margin: 0 }}>在线支付设置</Title>
          </div>
          <Tabs activeKey={activeTab} onChange={setActiveTab} items={tabItems} />
        </>
      ) : (
        <div style={{ animation: 'fadeIn 0.3s' }}>
          <div style={{ display: 'flex', alignItems: 'center', marginBottom: 16, gap: 16, flexWrap: 'wrap' }}>
            <Button icon={<ArrowLeftOutlined />} onClick={closeEditor}>返回</Button>
            <Title level={3} style={{ margin: 0 }}>
              配置支付渠道 · {editingMeta?.defaultName || editingId}
            </Title>
          </div>

          <div style={{ maxWidth: 760, width: '100%' }}>
            <Form form={formDisplay} layout="vertical" autoComplete="off">
              <Divider plain style={{ margin: '8px 0 16px' }}>用户端展示</Divider>

              <div style={{
                display: 'flex',
                flexWrap: 'wrap',
                alignItems: 'center',
                gap: 24,
                marginBottom: 16,
              }}>
                <Form.Item label="启用该渠道" name="enabled" valuePropName="checked" style={{ marginBottom: 0 }}>
                  <Switch />
                </Form.Item>
                {editingId === 'allinpay' && (
                  <>
                    <Form.Item
                      label="微信子渠道"
                      name="allinpay_wechat_enabled"
                      valuePropName="checked"
                      style={{ marginBottom: 0 }}
                    >
                      <Switch />
                    </Form.Item>
                    <Form.Item
                      label="支付宝子渠道"
                      name="allinpay_alipay_enabled"
                      valuePropName="checked"
                      style={{ marginBottom: 0 }}
                    >
                      <Switch />
                    </Form.Item>
                  </>
                )}
                <Form.Item
                  label="排序权重"
                  name="sort_order"
                  style={{ marginBottom: 0 }}
                >
                  <InputNumber min={0} max={9999} style={{ width: 100 }} placeholder="0" />
                </Form.Item>
              </div>

              <Row gutter={16}>
                <Col xs={24} sm={12}>
                  <Form.Item
                    label={
                      <Space size={4}>
                        <span>{editingMeta?.defaultName || editingId}</span>
                        <Text type="secondary" style={{ fontSize: 12, fontWeight: 'normal' }}>（中文副标题）</Text>
                      </Space>
                    }
                    name="subtitle"
                    extra={`留空使用默认：${editingMeta?.defaultSubtitle || '无'}`}
                    style={{ marginBottom: 16 }}
                  >
                    <Input placeholder={editingMeta?.defaultSubtitle} allowClear maxLength={32} />
                  </Form.Item>
                </Col>
                <Col xs={24} sm={12}>
                  <Form.Item
                    label={
                      <Space size={4}>
                        <span>{editingMeta?.defaultNameEn || editingMeta?.defaultName || editingId}</span>
                        <Text type="secondary" style={{ fontSize: 12, fontWeight: 'normal' }}>（英文副标题）</Text>
                      </Space>
                    }
                    name="subtitle_en"
                    extra={`留空使用默认：${editingMeta?.defaultSubtitleEn || 'None'}`}
                    style={{ marginBottom: 16 }}
                  >
                    <Input placeholder={editingMeta?.defaultSubtitleEn} allowClear maxLength={32} />
                  </Form.Item>
                </Col>
              </Row>

              <Form.Item label="Logo 图片 URL" name="logo_url" extra="留空则使用系统默认图标" style={{ marginBottom: 16 }}>
                <Input placeholder="https://..." allowClear />
              </Form.Item>
            </Form>

            <Form form={formGateway} layout="vertical" autoComplete="off">
              <Divider plain>网关参数</Divider>
              {renderGatewayFields()}
            </Form>

            <Space style={{ marginTop: 8, marginBottom: 24 }}>
              <Button type="primary" loading={savingDrawer} onClick={onSaveDrawer} size="large" style={{ borderRadius: 8 }}>
                保存配置
              </Button>
              <Button onClick={closeEditor} size="large" style={{ borderRadius: 8 }}>取消</Button>
            </Space>
          </div>
        </div>
      )}
    </Card>
  );
};

export default PaymentSettings;
