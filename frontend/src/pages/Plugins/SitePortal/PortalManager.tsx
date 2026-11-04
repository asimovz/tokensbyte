/*
 * tokensbyte opensource
 * (c) 2026 tokensbyte.ai
 * @copyright      Copyright netbcloud/wstianxia 
 * @license        MIT (https://www.tokensbyte.ai/)
 */

import React, { useState, useEffect } from 'react';
import { Typography, Input, Switch, Button, Divider, Spin, App, Space, Tag, Alert } from 'antd';
import { SaveOutlined, EyeOutlined, ThunderboltOutlined, PlusOutlined, DeleteOutlined, LinkOutlined, CopyOutlined } from '@ant-design/icons';
import { LayoutDashboard, Code, ShieldCheck, PanelBottom, FileCode } from 'lucide-react';
import request from '../../../utils/request';
import { useThemeStore } from '../../../store/theme';
import whatsTokenHomepageHtml from './whats-token-homepage.html?raw';

const { Text, Title } = Typography;
const { TextArea } = Input;

class ErrorBoundary extends React.Component<{ children: React.ReactNode }, { hasError: boolean; error: any }> {
  constructor(props: any) {
    super(props);
    this.state = { hasError: false, error: null };
  }

  static getDerivedStateFromError(error: any) {
    return { hasError: true, error };
  }

  componentDidCatch(error: any, errorInfo: any) {
    console.error("ErrorBoundary caught an error", error, errorInfo);
  }

  render() {
    if (this.state.hasError) {
      return (
        <div style={{ padding: 20, background: '#fff1f0', border: '1px solid #ffa39e', borderRadius: 8 }}>
          <Title level={5} style={{ color: '#ff4d4f', marginTop: 0 }}>配置面板加载失败</Title>
          <Text type="secondary" style={{ fontSize: 13, display: 'block', marginBottom: 12 }}>
            渲染该配置栏目时发生运行时错误。这通常是由于旧配置数据结构不兼容导致的，您可以点击下方按钮重试，或联系管理员排查。
          </Text>
          <pre style={{
            background: '#fafafa',
            padding: 12,
            borderRadius: 5,
            border: '1px solid rgba(0,0,0,0.06)',
            color: '#ff4d4f',
            fontFamily: 'monospace',
            fontSize: 12,
            overflowX: 'auto',
            maxHeight: 250
          }}>
            {this.state.error?.stack || this.state.error?.toString()}
          </pre>
          <Button size="small" type="primary" danger onClick={() => this.setState({ hasError: false, error: null })}>
            重试
          </Button>
        </div>
      );
    }
    return this.props.children;
  }
}

const DEMO_HTML = whatsTokenHomepageHtml;

const DEFAULT_NAV_ITEMS = [
  { label: '平台优势|Platform Advantages', path: '#features', enabled: true, key: 'features' },
  { label: '核心功能|Core Features', path: '#carousel', enabled: true, key: 'carousel' },
  { label: '模型矩阵|Model Matrix', path: '#models', enabled: true, key: 'models' },
  { label: '接入指南|Integration Guide', path: '#integration', enabled: true, key: 'integration' },
  { label: '模型广场|Model Marketplace', path: '/home/models', enabled: true, key: 'marketplace' },
];

type MenuKey = 'custom_homepage' | 'nav' | 'static_gen' | 'other' | 'footer';

const PortalManager: React.FC = () => {
  const { themeMode } = useThemeStore();
  const _isLight = themeMode === 'light';
  const { message } = App.useApp();
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [activeMenu, setActiveMenu] = useState<MenuKey>('nav');
  const [saveCooldowns, setSaveCooldowns] = useState<Record<string, number>>({});
  // Config states
  const [navConfig, setNavConfig] = useState<any>({});
  const [footerConfig, setFooterConfig] = useState<any>({});
  const [customScripts, setCustomScripts] = useState<any>({});
  const [seoConfig, setSeoConfig] = useState<any>({});
  const [staticGenConfig, setStaticGenConfig] = useState<any>({ manual_mode: false });
  const [generateLog, setGenerateLog] = useState<any[]>([]);
  const [generating, setGenerating] = useState(false);
  const [generatedLinks, setGeneratedLinks] = useState<{ label: string; path: string }[]>([]);
  const [customHomepage, setCustomHomepage] = useState<any>({ enabled: false, html: '' });

  useEffect(() => { fetchConfig(); }, []);

  useEffect(() => {
    const timer = setInterval(() => {
      setSaveCooldowns(prev => {
        const next = { ...prev };
        let changed = false;
        for (const key in next) {
          if (next[key] > 0) {
            next[key] -= 1;
            changed = true;
          } else {
            delete next[key];
            changed = true;
          }
        }
        return changed ? next : prev;
      });
    }, 1000);
    return () => clearInterval(timer);
  }, []);

  const fetchConfig = async () => {
    try {
      setLoading(true);
      const res = await (request.get('/plugins/site-portal/portal-config') as Promise<any>);
      if (res.nav_config) setNavConfig(res.nav_config);
      if (res.footer_config) setFooterConfig(res.footer_config);
      if (res.custom_scripts) setCustomScripts(res.custom_scripts);
      if (res.seo_config) setSeoConfig(res.seo_config);
      if (res.static_gen_config) setStaticGenConfig(res.static_gen_config);
      if (res.generate_log) setGenerateLog(res.generate_log);
      if (res.custom_homepage) {
        setCustomHomepage(res.custom_homepage);
        if (res.custom_homepage.enabled) {
          setActiveMenu('custom_homepage');
        }
      }
    } catch {
      message.error('加载门户配置失败');
    } finally {
      setLoading(false);
    }
  };
  const handleSave = async (section: string, data: any) => {
    if (saveCooldowns[section]) return;
    try {
      setSaving(true);
      await request.post('/plugins/site-portal/portal-config', { section, data });
      setSaveCooldowns(prev => ({ ...prev, [section]: 3 }));
      message.success('配置已保存');
    } catch {
      message.error('保存失败');
    } finally {
      setSaving(false);
    }
  };

  const handleSaveAllNav = async () => {
    if (saveCooldowns['nav']) return;
    try {
      setSaving(true);
      await request.post('/plugins/site-portal/portal-config', { section: 'nav', data: navConfig });
      await request.post('/plugins/site-portal/portal-config', { section: 'seo', data: seoConfig });
      setSaveCooldowns(prev => ({ ...prev, 'nav': 3 }));
      message.success('导航配置已保存');
    } catch {
      message.error('保存失败');
    } finally {
      setSaving(false);
    }
  };

  const handleGenerate = async (scope: string, columns?: string[]) => {
    try {
      setGenerating(true);
      setGeneratedLinks([]);
      const res = await (request.post('/plugins/site-portal/generate', { scope, columns }) as Promise<any>);
      message.success(res.message || '生成完成');
      if (res.generated_paths && Array.isArray(res.generated_paths)) {
        setGeneratedLinks(res.generated_paths);
      }
      fetchConfig();
    } catch (err: any) {
      const data = err?.response?.data;
      const detail =
        data?.error?.message || data?.message || (typeof data?.error === 'string' ? data.error : undefined) || err?.message || '生成失败';
      message.error(typeof detail === 'string' ? detail : '生成失败');
    } finally {
      setGenerating(false);
    }
  };

  const cardStyle = {
    background: _isLight ? '#fff' : '#141414',
    borderRadius: 8,
    border: _isLight ? '1px solid rgba(0,0,0,0.08)' : '1px solid rgba(255,255,255,0.08)',
    padding: '20px',
    marginBottom: 16,
  };

  const labelStyle = { color: _isLight ? 'rgba(0,0,0,0.65)' : 'rgba(255,255,255,0.65)', fontSize: 13, display: 'block' as const, marginBottom: 6 };

  // ─── Left Menu ───
  const menuItems: { key: MenuKey; icon?: React.ReactNode; label: string; isTitle?: boolean; isSub?: boolean }[] = [
    { key: 'custom_homepage', icon: <FileCode size={16} strokeWidth={1.5} />, label: '自定义主页' },
    { key: 'nav', icon: <LayoutDashboard size={16} strokeWidth={1.5} />, label: '导航管理' },
    { key: 'footer', icon: <PanelBottom size={16} strokeWidth={1.5} />, label: '底部管理' },
    { key: 'static_gen', icon: <Code size={16} strokeWidth={1.5} />, label: '静态生成' },
    { key: 'other', icon: <ShieldCheck size={16} strokeWidth={1.5} />, label: '其他配置' },
  ];

  // ─── Right Panel Content ───

  const renderNav = () => (
    <div>
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 16 }}>
        <Title level={5} style={{ margin: 0, color: _isLight ? '#1f2937' : '#fff' }}>导航管理</Title>
        <Button type="primary" icon={<SaveOutlined />} loading={saving} disabled={(saveCooldowns['nav'] || 0) > 0} onClick={handleSaveAllNav}>
          {(saveCooldowns['nav'] || 0) > 0 ? `已保存 (${saveCooldowns['nav']}s)` : '保存导航配置'}
        </Button>
      </div>
      <div style={cardStyle}>
        <Text style={labelStyle}>Logo 图片 URL（留空则只显示文字）</Text>
        <Input value={navConfig.logo_url || ''} onChange={e => setNavConfig({ ...navConfig, logo_url: e.target.value })} placeholder="https://cdn.example.com/logo.png" style={{ marginBottom: 12 }} />
        <Text style={labelStyle}>Logo 点击跳转链接</Text>
        <Input value={navConfig.logo_link || ''} onChange={e => setNavConfig({ ...navConfig, logo_link: e.target.value })} placeholder="例如：/home 或 https://..." style={{ marginBottom: 12 }} />
        <Text style={labelStyle}>Logo 文字</Text>
        <Input value={navConfig.logo_text || ''} onChange={e => setNavConfig({ ...navConfig, logo_text: e.target.value })} placeholder="Tkeapi" style={{ marginBottom: 12 }} />
        <Divider style={{ borderColor: _isLight ? 'rgba(0,0,0,0.06)' : 'rgba(255,255,255,0.06)', margin: '14px 0' }} />
        <Text style={labelStyle}>登录按钮文字 / 链接</Text>
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 8, marginBottom: 12 }}>
          <Input value={navConfig.cta_text || '登录'} onChange={e => setNavConfig({ ...navConfig, cta_text: e.target.value })} />
          <Input value={navConfig.cta_link || '/login'} onChange={e => setNavConfig({ ...navConfig, cta_link: e.target.value })} />
        </div>
        <Text style={labelStyle}>注册按钮文字 / 链接</Text>
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 8 }}>
          <Input value={navConfig.register_text || '注册'} onChange={e => setNavConfig({ ...navConfig, register_text: e.target.value })} />
          <Input value={navConfig.register_link || '/register'} onChange={e => setNavConfig({ ...navConfig, register_link: e.target.value })} />
        </div>
      </div>

      {/* 顶部导航菜单 */}
      <div style={cardStyle}>
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 4 }}>
          <Text strong style={{ color: _isLight ? '#1f2937' : '#fff', fontSize: 14 }}>顶部导航菜单</Text>
          <Space size={8}>
            <Button size="small" onClick={() => {
              setNavConfig({ ...navConfig, items: DEFAULT_NAV_ITEMS.map(item => ({ ...item })) });
            }}>恢复默认五项</Button>
            <Button size="small" icon={<PlusOutlined />} onClick={() => {
              const items = [...(navConfig.items || []), { label: '新菜单|New Menu', path: '#features', enabled: true, key: `item_${Date.now()}` }];
              setNavConfig({ ...navConfig, items });
            }}>添加栏目</Button>
          </Space>
        </div>
        <div style={{ marginBottom: 16 }}>
          <Text type="secondary" style={{ fontSize: 13 }}>💡 提示：站内锚点（如 #features）会在当前首页平滑滚动；也支持 /home/models 等页面链接。名称填写格式为 <Text code>中文|English</Text>（如 <Text code>帮助中心|Help Center</Text>）。</Text>
        </div>
        {(navConfig.items || []).map((item: any, idx: number) => (
          <div key={idx} style={{ display: 'grid', gridTemplateColumns: 'auto 1fr 1fr 1.2fr auto', gap: 8, marginBottom: 8, alignItems: 'center' }}>
            <Switch
              size="small"
              checked={item.enabled !== false}
              onChange={v => {
                const items = [...navConfig.items];
                items[idx] = { ...item, enabled: v };
                setNavConfig({ ...navConfig, items });
              }}
            />
            <Input
              value={item.label}
              onChange={e => {
                const items = [...navConfig.items];
                items[idx] = { ...item, label: e.target.value };
                setNavConfig({ ...navConfig, items });
              }}
              placeholder="菜单名称（如：平台优势|Platform Advantages）"
            />
            <Input
              value={item.path}
              onChange={e => {
                const items = [...navConfig.items];
                items[idx] = { ...item, path: e.target.value };
                setNavConfig({ ...navConfig, items });
              }}
              placeholder="锚点或链接（如 #features、/home/models）"
            />
            <Input
              value={item.icon || ''}
              onChange={e => {
                const items = [...navConfig.items];
                items[idx] = { ...item, icon: e.target.value };
                setNavConfig({ ...navConfig, items });
              }}
              placeholder="图标 SVG（风格化页）"
            />
            <Button
              size="small"
              danger
              icon={<DeleteOutlined />}
              onClick={() => {
                const items = navConfig.items.filter((_: any, i: number) => i !== idx);
                setNavConfig({ ...navConfig, items });
              }}
            />
          </div>
        ))}
        {(!navConfig.items || navConfig.items.length === 0) && (
          <Text style={{ color: _isLight ? 'rgba(0,0,0,0.25)' : 'rgba(255,255,255,0.25)', fontSize: 13 }}>暂无导航菜单，点击添加</Text>
        )}
      </div>
      {/* SEO */}
      <div style={cardStyle}>
        <Text strong style={{ color: _isLight ? '#1f2937' : '#fff', fontSize: 14, display: 'block', marginBottom: 12 }}>SEO 元信息</Text>
        <Text style={labelStyle}>页面标题 (meta title)</Text>
        <Input value={seoConfig.meta_title || ''} onChange={e => setSeoConfig({ ...seoConfig, meta_title: e.target.value })} placeholder="站点标题" style={{ marginBottom: 8 }} />
        <Text style={labelStyle}>页面描述 (meta description)</Text>
        <Input value={seoConfig.meta_description || ''} onChange={e => setSeoConfig({ ...seoConfig, meta_description: e.target.value })} placeholder="站点描述" style={{ marginBottom: 8 }} />
        <Text style={labelStyle}>关键词 (meta keywords)</Text>
        <Input value={seoConfig.meta_keywords || ''} onChange={e => setSeoConfig({ ...seoConfig, meta_keywords: e.target.value })} placeholder="AI, API, 模型" />
      </div>
    </div>
  );

  const renderFooter = () => (
    <div>
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 16 }}>
        <Title level={5} style={{ margin: 0, color: _isLight ? '#1f2937' : '#fff' }}>底部管理</Title>
        <Space>
          <Button type="primary" icon={<SaveOutlined />} loading={saving} disabled={(saveCooldowns['footer'] || 0) > 0} onClick={() => handleSave('footer', footerConfig)}>
            {(saveCooldowns['footer'] || 0) > 0 ? `已保存 (${saveCooldowns['footer']}s)` : '保存'}
          </Button>
        </Space>
      </div>
      <div style={cardStyle}>
        <Text strong style={{ color: _isLight ? '#1f2937' : '#fff', fontSize: 14, display: 'block', marginBottom: 12 }}>品牌与简介</Text>
        <Text style={labelStyle}>底部品牌名称（支持 中文|English）</Text>
        <Input value={footerConfig.brand_name || ''} onChange={e => setFooterConfig({ ...footerConfig, brand_name: e.target.value })} placeholder="Tkeapi" style={{ marginBottom: 8 }} />
        <Text style={labelStyle}>品牌简介（支持 中文|English）</Text>
        <Input.TextArea value={footerConfig.description || ''} onChange={e => setFooterConfig({ ...footerConfig, description: e.target.value })} rows={3} style={{ marginBottom: 8 }} />
        <Divider style={{ margin: '16px 0' }} />
        <Text strong style={{ color: _isLight ? '#1f2937' : '#fff', fontSize: 14, display: 'block', marginBottom: 12 }}>产品与服务</Text>
        <Text style={labelStyle}>栏目标题（支持 中文|English）</Text>
        <Input value={footerConfig.links_title || ''} onChange={e => setFooterConfig({ ...footerConfig, links_title: e.target.value })} placeholder="产品与服务|Products & Services" style={{ marginBottom: 8 }} />
        <Text style={labelStyle}>底部链接列表（前台开启/关闭控制）</Text>
        {(footerConfig.links || []).map((link: any, idx: number) => (
          <div key={idx} style={{ display: 'grid', gridTemplateColumns: 'auto 1fr 1fr 32px', gap: 8, marginBottom: 8, alignItems: 'center' }}>
            <Switch
              size="small"
              checked={link.enabled !== false}
              onChange={v => {
                const links = [...(footerConfig.links || [])];
                links[idx] = { ...link, enabled: v };
                setFooterConfig({ ...footerConfig, links });
              }}
            />
            <Input value={link.label || ''} onChange={e => { const links = [...(footerConfig.links || [])]; links[idx] = { ...link, label: e.target.value }; setFooterConfig({ ...footerConfig, links }); }} placeholder="名称，如 技术优势|Technical Advantages" />
            <Input value={link.path || ''} onChange={e => { const links = [...(footerConfig.links || [])]; links[idx] = { ...link, path: e.target.value }; setFooterConfig({ ...footerConfig, links }); }} placeholder="锚点或链接，如 #features" />
            <Button danger icon={<DeleteOutlined />} onClick={() => setFooterConfig({ ...footerConfig, links: (footerConfig.links || []).filter((_: any, i: number) => i !== idx) })} />
          </div>
        ))}
        <Button size="small" icon={<PlusOutlined />} onClick={() => setFooterConfig({ ...footerConfig, links: [...(footerConfig.links || []), { label: '新链接|New Link', path: '#features', enabled: true }] })}>添加底部链接</Button>
        <Divider style={{ margin: '16px 0' }} />
        <Text strong style={{ color: _isLight ? '#1f2937' : '#fff', fontSize: 14, display: 'block', marginBottom: 12 }}>开发者资讯</Text>
        <Text style={labelStyle}>栏目标题（支持 中文|English）</Text>
        <Input value={footerConfig.news_title || ''} onChange={e => setFooterConfig({ ...footerConfig, news_title: e.target.value })} placeholder="开发者资讯|Developer News" style={{ marginBottom: 8 }} />
        <Text style={labelStyle}>栏目说明（支持 中文|English）</Text>
        <Input.TextArea value={footerConfig.news_description || ''} onChange={e => setFooterConfig({ ...footerConfig, news_description: e.target.value })} rows={3} />
        <Divider style={{ margin: '16px 0' }} />
        <Text strong style={{ color: _isLight ? '#1f2937' : '#fff', fontSize: 14, display: 'block', marginBottom: 12 }}>公司与法律信息</Text>
        <Text style={labelStyle}>版权信息</Text>
        <Input value={footerConfig.copyright || ''} onChange={e => setFooterConfig({ ...footerConfig, copyright: e.target.value })} placeholder="© 2026 TkeAPI. All rights reserved." style={{ marginBottom: 8 }} />
        <Text style={labelStyle}>公司名称</Text>
        <Input value={footerConfig.company_name || ''} onChange={e => setFooterConfig({ ...footerConfig, company_name: e.target.value })} style={{ marginBottom: 8 }} />
        <Text style={labelStyle}>公司地址</Text>
        <Input value={footerConfig.company_address || ''} onChange={e => setFooterConfig({ ...footerConfig, company_address: e.target.value })} style={{ marginBottom: 8 }} />
        <Text style={labelStyle}>备案号（可选）</Text>
        <Input value={footerConfig.icp_number || ''} onChange={e => setFooterConfig({ ...footerConfig, icp_number: e.target.value })} placeholder="京ICP备xxxxxxxx号" style={{ marginBottom: 8 }} />
        
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 6 }}>
          <Text style={labelStyle}>服务条款：名称 / 链接</Text>
          <Space size={6}>
            <Text style={{ fontSize: 12, color: _isLight ? 'rgba(0,0,0,0.45)' : 'rgba(255,255,255,0.45)' }}>前台展示开关</Text>
            <Switch
              size="small"
              checked={footerConfig.terms_enabled !== false}
              onChange={v => setFooterConfig({ ...footerConfig, terms_enabled: v })}
            />
          </Space>
        </div>
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 8, marginBottom: 8 }}>
          <Input value={footerConfig.terms_text || ''} onChange={e => setFooterConfig({ ...footerConfig, terms_text: e.target.value })} placeholder="服务条款|Terms of Service" disabled={footerConfig.terms_enabled === false} />
          <Input value={footerConfig.terms_link || ''} onChange={e => setFooterConfig({ ...footerConfig, terms_link: e.target.value })} placeholder="/legal/terms" disabled={footerConfig.terms_enabled === false} />
        </div>

        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 6 }}>
          <Text style={labelStyle}>隐私政策：名称 / 链接</Text>
          <Space size={6}>
            <Text style={{ fontSize: 12, color: _isLight ? 'rgba(0,0,0,0.45)' : 'rgba(255,255,255,0.45)' }}>前台展示开关</Text>
            <Switch
              size="small"
              checked={footerConfig.privacy_enabled !== false}
              onChange={v => setFooterConfig({ ...footerConfig, privacy_enabled: v })}
            />
          </Space>
        </div>
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 8 }}>
          <Input value={footerConfig.privacy_text || ''} onChange={e => setFooterConfig({ ...footerConfig, privacy_text: e.target.value })} placeholder="隐私政策|Privacy Policy" disabled={footerConfig.privacy_enabled === false} />
          <Input value={footerConfig.privacy_link || ''} onChange={e => setFooterConfig({ ...footerConfig, privacy_link: e.target.value })} placeholder="/legal/privacy" disabled={footerConfig.privacy_enabled === false} />
        </div>
      </div>
    </div>
  );

  const handleCopyLink = (path: string) => {
    const fullUrl = `${window.location.origin}${path}`;
    navigator.clipboard.writeText(fullUrl).then(() => {
      message.success('链接已复制到剪贴板');
    }).catch(() => {
      message.error('复制失败');
    });
  };

  const renderStaticGen = () => (
    <div>
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 16 }}>
        <Title level={5} style={{ margin: 0, color: _isLight ? '#1f2937' : '#fff' }}>静态 HTML 生成</Title>
      </div>
      <Alert type="info" showIcon title="生成后的静态 HTML 文件将部署到 /portal 路径，便于搜索引擎抓取和 SEO/GEO 优化" style={{ marginBottom: 16 }} />

      <div style={cardStyle}>
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
          <div>
            <Text strong style={{ color: _isLight ? '#1f2937' : '#fff', fontSize: 14, display: 'block', marginBottom: 4 }}>手动静态 HTML 生成模式</Text>
            <Text style={{ color: _isLight ? 'rgba(0,0,0,0.45)' : 'rgba(255,255,255,0.45)', fontSize: 12 }}>
              默认关闭。在关闭状态下，每次修改配置并保存后，后台均会自动实时生成前台 HTML 页面，无需手动生成。
            </Text>
          </div>
          <Switch 
            checked={staticGenConfig.manual_mode === true} 
            onChange={async (checked) => {
              const newConfig = { manual_mode: checked };
              setStaticGenConfig(newConfig);
              try {
                await request.post('/plugins/site-portal/portal-config', { section: 'static_gen', data: newConfig });
                message.success(checked ? '已开启手动静态 HTML 生成模式' : '已关闭手动模式，转为实时自动更新数据');
                fetchConfig();
              } catch {
                message.error('保存设置失败');
              }
            }}
          />
        </div>
      </div>

      {!staticGenConfig.manual_mode ? (
        <Alert 
          type="success" 
          showIcon 
          title="当前为「自动更新」：保存门户/风格配置后会后台自动生成 /portal 静态页，无需手动点击。" 
          style={{ marginBottom: 16 }} 
        />
      ) : (
        <Alert 
          type="warning" 
          showIcon 
          title="当前为「手动生成」：保存配置后不会自动更新，请点击下方按钮生成静态页。" 
          style={{ marginBottom: 16 }} 
        />
      )}

      <Alert
        type="info"
        showIcon
        style={{ marginBottom: 16 }}
        title="路径说明"
        description={
          <span>
            互动模型广场走 React 路由 <Text code>/home/models</Text>；下方「SEO 模型页」生成的是 <Text code>/portal/models</Text> 静态页（供爬虫/外链），两者独立。
          </span>
        }
      />

      <div style={cardStyle}>
        <Text strong style={{ color: _isLight ? '#1f2937' : '#fff', fontSize: 14, display: 'block', marginBottom: 16 }}>快捷操作</Text>
        <div style={{ display: 'flex', flexWrap: 'wrap', gap: 12 }}>
          <Button
            disabled={staticGenConfig.manual_mode !== true}
            type="primary"
            icon={<ThunderboltOutlined />}
            loading={generating}
            onClick={() => handleGenerate('all')}
          >
            全站生成
          </Button>
          <Button
            disabled={staticGenConfig.manual_mode !== true}
            loading={generating}
            onClick={() => handleGenerate('home')}
          >
            首页
          </Button>
          <Button
            disabled={staticGenConfig.manual_mode !== true}
            loading={generating}
            onClick={() => handleGenerate('columns', ['models'])}
          >
            SEO 模型页更新
          </Button>
          <Button
            disabled={staticGenConfig.manual_mode !== true}
            loading={generating}
            onClick={() => handleGenerate('columns', ['contact', 'about'])}
          >
            联系/关于
          </Button>
        </div>
      </div>

      {/* 生成后的快捷链接 */}
      {generatedLinks.length > 0 && (
        <div style={{
          ...cardStyle,
          background: _isLight ? 'linear-gradient(135deg, #f0fdf4, #dcfce7)' : 'linear-gradient(135deg, #052e16, #064e3b)',
          border: _isLight ? '1px solid #86efac' : '1px solid #065f46',
        }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 12 }}>
            <LinkOutlined style={{ color: '#22c55e', fontSize: 16 }} />
            <Text strong style={{ color: _isLight ? '#166534' : '#86efac', fontSize: 14 }}>生成完成 - 快捷访问链接</Text>
          </div>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
            {generatedLinks.map((link, idx) => (
              <div key={idx} style={{
                display: 'flex', alignItems: 'center', justifyContent: 'space-between',
                padding: '8px 12px', borderRadius: 6,
                background: _isLight ? 'rgba(255,255,255,0.7)' : 'rgba(0,0,0,0.2)',
              }}>
                <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                  <Tag color="green" style={{ margin: 0 }}>{link.label}</Tag>
                  <a
                    href={link.path}
                    target="_blank"
                    rel="noopener noreferrer"
                    style={{ color: '#1677ff', fontSize: 13, textDecoration: 'none' }}
                  >
                    {window.location.origin}{link.path}
                  </a>
                </div>
                <Space size={4}>
                  <Button
                    type="text"
                    size="small"
                    icon={<CopyOutlined />}
                    onClick={() => handleCopyLink(link.path)}
                    style={{ color: _isLight ? 'rgba(0,0,0,0.45)' : 'rgba(255,255,255,0.45)' }}
                  />
                  <Button
                    type="link"
                    size="small"
                    icon={<EyeOutlined />}
                    onClick={() => window.open(link.path, '_blank')}
                  >
                    查看
                  </Button>
                </Space>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* 生成日志 */}
      <div style={cardStyle}>
        <Text strong style={{ color: _isLight ? '#1f2937' : '#fff', fontSize: 14, display: 'block', marginBottom: 12 }}>最近生成记录</Text>
        {generateLog.length > 0 ? generateLog.slice(0, 10).map((log: any, idx: number) => (
          <div key={idx} style={{ display: 'flex', alignItems: 'center', gap: 12, padding: '8px 0', borderBottom: _isLight ? '1px solid rgba(0,0,0,0.04)' : '1px solid rgba(255,255,255,0.04)' }}>
            <Text style={{ color: _isLight ? 'rgba(0,0,0,0.35)' : 'rgba(255,255,255,0.35)', fontSize: 12, minWidth: 140 }}>{log.time}</Text>
            <Tag style={{ margin: 0 }}>{log.scope === 'all' ? '全站' : log.scope}</Tag>
            <Text style={{ color: _isLight ? 'rgba(0,0,0,0.65)' : 'rgba(255,255,255,0.65)', fontSize: 13 }}>{(log.pages || []).join('、')}</Text>
          </div>
        )) : (
          <Text style={{ color: _isLight ? 'rgba(0,0,0,0.25)' : 'rgba(255,255,255,0.25)', fontSize: 13 }}>暂无生成记录</Text>
        )}
      </div>
    </div>
  );

  const renderOther = () => (
    <div>
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 16 }}>
        <Title level={5} style={{ margin: 0, color: _isLight ? '#1f2937' : '#fff' }}>其他配置</Title>
        <Button type="primary" icon={<SaveOutlined />} loading={saving} disabled={(saveCooldowns['scripts'] || 0) > 0} onClick={() => handleSave('scripts', customScripts)}>
          {(saveCooldowns['scripts'] || 0) > 0 ? `已保存 (${saveCooldowns['scripts']}s)` : '保存'}
        </Button>
      </div>
      <div style={cardStyle}>
        <Text strong style={{ color: _isLight ? '#1f2937' : '#fff', fontSize: 14, display: 'block', marginBottom: 4 }}>客服代码</Text>
        <Text style={{ color: _isLight ? 'rgba(0,0,0,0.35)' : 'rgba(255,255,255,0.35)', fontSize: 12, display: 'block', marginBottom: 8 }}>
          输入的 JS 代码将自动加载到所有门户页面（注入到 &lt;/body&gt; 前）
        </Text>
        <TextArea rows={6} value={customScripts.customer_service || ''} onChange={e => setCustomScripts({ ...customScripts, customer_service: e.target.value })}
          placeholder={'<script>\n// 客服系统 JS 代码\n</script>'} style={{ fontFamily: 'monospace', fontSize: 12 }} />
      </div>
      <div style={cardStyle}>
        <Text strong style={{ color: _isLight ? '#1f2937' : '#fff', fontSize: 14, display: 'block', marginBottom: 4 }}>统计代码</Text>
        <Text style={{ color: _isLight ? 'rgba(0,0,0,0.35)' : 'rgba(255,255,255,0.35)', fontSize: 12, display: 'block', marginBottom: 8 }}>
          输入的 JS 代码将自动加载到所有门户页面（注入到 &lt;head&gt; 中）
        </Text>
        <TextArea rows={6} value={customScripts.analytics || ''} onChange={e => setCustomScripts({ ...customScripts, analytics: e.target.value })}
          placeholder={'<!-- Google Analytics -->\n<script async src="https://..."></script>'} style={{ fontFamily: 'monospace', fontSize: 12 }} />
      </div>
    </div>
  );

  const renderCustomHomepage = () => (
    <div>
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 16 }}>
        <Title level={5} style={{ margin: 0, color: _isLight ? '#1f2937' : '#fff' }}>自定义主页</Title>
        <Button type="primary" icon={<SaveOutlined />} loading={saving} disabled={(saveCooldowns['custom_homepage'] || 0) > 0} onClick={() => handleSave('custom_homepage', customHomepage)}>
          {(saveCooldowns['custom_homepage'] || 0) > 0 ? `已保存 (${saveCooldowns['custom_homepage']}s)` : '保存'}
        </Button>
      </div>

      <div style={cardStyle}>
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 16 }}>
          <div>
            <Text strong style={{ color: _isLight ? '#1f2937' : '#fff', fontSize: 14, display: 'block' }}>启用自定义主页</Text>
            <Text style={{ color: _isLight ? 'rgba(0,0,0,0.45)' : 'rgba(255,255,255,0.45)', fontSize: 12 }}>
              关闭：使用托管首页（导航/底部生效），或在「风格选择」中开启风格化 Tera 首页。开启：直接渲染粘贴的 HTML（导航/底部/风格均不注入）。
            </Text>
          </div>
          <Switch
            checked={customHomepage.enabled}
            loading={saving}
            onChange={async v => {
              const newHtml = (v && !customHomepage.html) ? DEMO_HTML : customHomepage.html;
              const newCustomHomepage = { ...customHomepage, enabled: v, html: newHtml };
              setCustomHomepage(newCustomHomepage);
              if (v) {
                setActiveMenu('custom_homepage');
              }
              try {
                setSaving(true);
                await request.post('/plugins/site-portal/portal-config', { section: 'custom_homepage', data: newCustomHomepage });
                message.success(v ? '已开启自定义主页' : '已关闭自定义主页');
              } catch {
                message.error('设置失败');
              } finally {
                setSaving(false);
              }
            }}
          />
        </div>
      </div>

      {customHomepage.enabled && (
        <>
          <Alert
            title="自定义主页已启用"
            description="首页将直接显示粘贴的 HTML。导航/底部/风格选择不会注入到该 HTML；配置仍会保留，关闭开关即可恢复托管或风格化首页。"
            type="warning"
            showIcon
            style={{ marginBottom: 16 }}
          />

          <div style={cardStyle}>
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 8 }}>
              <div>
                <Text strong style={{ color: _isLight ? '#1f2937' : '#fff', fontSize: 14 }}>HTML 代码</Text>
                <Text style={{ color: _isLight ? 'rgba(0,0,0,0.35)' : 'rgba(255,255,255,0.35)', fontSize: 12, marginLeft: 8 }}>
                  粘贴完整的 HTML 页面代码（包含 &lt;html&gt;&lt;head&gt;&lt;body&gt; 等标签）
                </Text>
              </div>
              <Button size="small" onClick={() => {
                setCustomHomepage({ ...customHomepage, html: DEMO_HTML });
              }}>恢复默认演示模板</Button>
            </div>
            <TextArea
              rows={20}
              value={customHomepage.html || ''}
              onChange={e => setCustomHomepage({ ...customHomepage, html: e.target.value })}
              placeholder={'<!DOCTYPE html>\n<html lang="zh">\n<head>\n  <meta charset="UTF-8">\n  <meta name="viewport" content="width=device-width, initial-scale=1.0">\n  <title>我的自定义主页</title>\n  <style>\n    /* 您的样式 */\n  </style>\n</head>\n<body>\n  <!-- 您的内容 -->\n</body>\n</html>'}
              style={{
                fontFamily: 'ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace',
                fontSize: 12,
                lineHeight: '1.6',
                resize: 'vertical',
                minHeight: 400,
              }}
            />
            <div style={{ marginTop: 12, display: 'flex', alignItems: 'center', gap: 12 }}>
              <Text style={{ color: _isLight ? 'rgba(0,0,0,0.35)' : 'rgba(255,255,255,0.35)', fontSize: 12 }}>
                💡 提示：您可以使用 AI 生成完整的单页 HTML 代码，然后粘贴到这里。保存后即可通过门户首页查看效果。
              </Text>
            </div>
          </div>
        </>
      )}
    </div>
  );

  if (loading) return <div style={{ textAlign: 'center', padding: 60 }}><Spin /></div>;

  const panels: Record<MenuKey, () => React.ReactNode> = {
    custom_homepage: renderCustomHomepage,
    nav: renderNav,
    footer: renderFooter,
    static_gen: renderStaticGen,
    other: renderOther,
  };

  return (
    <div style={{ display: 'flex', gap: 16, minHeight: 500 }}>
      {/* Left Sidebar */}
      <div style={{
        width: 180, flexShrink: 0,
        background: _isLight ? '#fff' : '#141414',
        border: _isLight ? '1px solid rgba(0,0,0,0.08)' : '1px solid rgba(255,255,255,0.08)',
        borderRadius: 8, padding: '8px 0', alignSelf: 'flex-start', position: 'sticky', top: 80,
      }}>
        {menuItems.map(item => {
          if (item.isTitle) {
            return (
              <div key={item.key} style={{
                display: 'flex', alignItems: 'center', gap: 8,
                padding: '10px 16px 4px 16px', fontSize: 12, fontWeight: 600,
                color: _isLight ? 'rgba(0,0,0,0.45)' : 'rgba(255,255,255,0.45)',
              }}>
                {item.icon}
                {item.label}
              </div>
            );
          }
          return (
            <div
              key={item.key}
              onClick={() => setActiveMenu(item.key as MenuKey)}
              style={{
                display: 'flex', alignItems: 'center', gap: 8,
                padding: `10px 16px 10px ${item.isSub ? '32px' : '16px'}`, cursor: 'pointer', fontSize: 13, fontWeight: 500,
                color: activeMenu === item.key ? '#1677ff' : (_isLight ? 'rgba(0,0,0,0.65)' : 'rgba(255,255,255,0.65)'),
                background: activeMenu === item.key ? (_isLight ? 'rgba(22,119,255,0.06)' : 'rgba(22,119,255,0.08)') : 'transparent',
                borderRight: activeMenu === item.key ? '2px solid #1677ff' : '2px solid transparent',
                transition: 'all 0.15s',
              }}
            >
              {item.icon}
              {item.label}
            </div>
          );
        })}
      </div>

      {/* Right Panel */}
      <div style={{ flex: 1, minWidth: 0 }}>
        <ErrorBoundary key={activeMenu}>
          {panels[activeMenu]?.()}
        </ErrorBoundary>
      </div>
    </div>
  );
};

export default PortalManager;
