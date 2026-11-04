/*
 * tokensbyte opensource
 * (c) 2026 tokensbyte.ai
 * @copyright      Copyright netbcloud/wstianxia 
 * @license        MIT (https://www.tokensbyte.ai/)
 */

import React, { useEffect, useState } from 'react';
import { Progress, Spin, message } from 'antd';
import { GitCommit, User, Calendar, Tag as TagIcon, ChevronDown, ChevronUp, MonitorPlay } from 'lucide-react';
import request from '../../utils/request';
import { useThemeStore } from '../../store/theme';

interface Commit {
  index: number;
  is_current: boolean;
  version: string;
  hash: string;
  short_hash: string;
  author: string;
  date: string;
  message: string;
}

interface RuntimeInfo {
  instance_name: string;
  instance_id: string;
  status: string;
  role: string;
  cpu_percent: number;
  memory_percent: number;
  disk_percent: number;
  platform: string;
  started_at: string;
}

/** 与控制台仪表盘灰底（--dashboard / #f0f4f9）一致 */
const SITE_SURFACE = 'bg-dashboard';
const SITE_SURFACE_SOFT = 'bg-dashboard/80 dark:bg-muted/40';

const formatPct = (v: number) => {
  if (!Number.isFinite(v)) return '0%';
  const rounded = Math.round(v * 10) / 10;
  return `${rounded}%`;
};

const ringStroke = (pct: number, isLight: boolean) => {
  if (pct >= 90) return '#ef4444';
  if (pct >= 70) return '#f59e0b';
  return isLight ? '#16a34a' : '#4ade80';
};

const MetricRing: React.FC<{
  percent: number;
  isLight: boolean;
  label?: string;
  stacked?: boolean;
}> = ({ percent, isLight, label, stacked }) => {
  const pct = Math.max(0, Math.min(100, Number.isFinite(percent) ? percent : 0));
  return (
    <div className={stacked ? 'flex flex-col items-center gap-1' : 'flex items-center gap-1.5'}>
      {label && (
        <span className="text-[10px] text-muted-foreground leading-none">{label}</span>
      )}
      <div className={stacked ? 'flex flex-col items-center gap-0.5' : 'flex items-center gap-1.5'}>
        <Progress
          type="circle"
          percent={pct}
          size={stacked ? 30 : 22}
          strokeWidth={10}
          strokeColor={ringStroke(pct, isLight)}
          railColor={isLight ? '#e2e8f0' : 'rgba(255,255,255,0.1)'}
          format={() => null}
        />
        <span className={`tabular-nums text-foreground ${stacked ? 'text-[11px] font-medium' : 'text-xs'}`}>
          {formatPct(pct)}
        </span>
      </div>
    </div>
  );
};

const Badge: React.FC<{
  children: React.ReactNode;
  variant?: 'default' | 'secondary' | 'outline' | 'success';
}> = ({ children, variant = 'secondary' }) => {
  const styles = {
    default: 'border-transparent bg-primary text-primary-foreground',
    secondary: 'border-border bg-muted text-muted-foreground',
    outline: 'border-border bg-background text-foreground',
    success: 'border-transparent bg-emerald-500/10 text-emerald-700 dark:text-emerald-400',
  }[variant];
  return (
    <span className={`inline-flex items-center gap-1 rounded-md border px-1.5 py-0.5 text-[11px] font-medium leading-none transition-colors ${styles}`}>
      {children}
    </span>
  );
};

const Card: React.FC<{ children: React.ReactNode; className?: string }> = ({ children, className = '' }) => (
  <div className={`rounded-lg border border-border bg-card text-card-foreground shadow-sm ${className}`}>
    {children}
  </div>
);

const RuntimePanel: React.FC<{ runtime: RuntimeInfo; isLight: boolean }> = ({ runtime, isLight }) => {
  const online = runtime.status === 'online';

  return (
    <Card>

      {/* 手机端：紧凑卡片 */}
      <div className="md:hidden p-3">
        <div className={`rounded-md border border-border px-3 py-2.5 space-y-2.5 ${SITE_SURFACE_SOFT}`}>
          <div className="flex items-center justify-between gap-2">
            <div className="flex items-center gap-1.5 min-w-0">
              <span
                className={`w-1.5 h-1.5 rounded-full shrink-0 ${
                  online ? 'bg-emerald-500' : 'bg-muted-foreground/40'
                }`}
              />
              <div className="min-w-0">
                <div className="text-xs font-medium text-foreground truncate">
                  {runtime.instance_name || '-'}
                </div>
                <div className="text-[10px] font-mono text-muted-foreground truncate leading-tight">
                  {runtime.instance_id || '-'}
                </div>
              </div>
            </div>
            <div className="flex items-center gap-1 shrink-0">
              <Badge variant={online ? 'success' : 'secondary'}>
                <span className={`w-1 h-1 rounded-full ${online ? 'bg-emerald-500' : 'bg-muted-foreground'}`} />
                {online ? '在线' : (runtime.status || '未知')}
              </Badge>
              <Badge variant="outline">{runtime.role || 'master'}</Badge>
            </div>
          </div>

          <div className="grid grid-cols-3 gap-1.5 pt-2 border-t border-border">
            <div className={`rounded-md border border-border py-1.5 ${SITE_SURFACE}`}>
              <MetricRing percent={runtime.cpu_percent} isLight={isLight} label="CPU" stacked />
            </div>
            <div className={`rounded-md border border-border py-1.5 ${SITE_SURFACE}`}>
              <MetricRing percent={runtime.memory_percent} isLight={isLight} label="内存" stacked />
            </div>
            <div className={`rounded-md border border-border py-1.5 ${SITE_SURFACE}`}>
              <MetricRing percent={runtime.disk_percent} isLight={isLight} label="存储" stacked />
            </div>
          </div>

          <div className="space-y-1.5 pt-2 border-t border-border">
            <div className="flex items-center justify-between gap-2 text-[11px]">
              <span className="text-muted-foreground shrink-0">运行环境</span>
              <span className="font-mono text-foreground whitespace-nowrap">{runtime.platform || '-'}</span>
            </div>
            <div className="flex items-center justify-between gap-2 text-[11px]">
              <span className="text-muted-foreground shrink-0">启动时间</span>
              <span className="tabular-nums text-foreground whitespace-nowrap">{runtime.started_at || '-'}</span>
            </div>
          </div>
        </div>
      </div>

      {/* 桌面端：table 保证表头与数据列严格对齐 */}
      <div className="hidden md:block overflow-x-auto p-3">
        <div className={`rounded-md border border-border overflow-hidden ${SITE_SURFACE_SOFT}`}>
          <table className="w-full border-collapse text-left">
            <thead>
              <tr className="text-[11px] text-muted-foreground border-b border-border">
                <th className="font-normal px-2.5 py-1.5 whitespace-nowrap">实例</th>
                <th className="font-normal px-2.5 py-1.5 whitespace-nowrap">状态</th>
                <th className="font-normal px-2.5 py-1.5 whitespace-nowrap">角色</th>
                <th className="font-normal px-2.5 py-1.5 whitespace-nowrap">CPU</th>
                <th className="font-normal px-2.5 py-1.5 whitespace-nowrap">内存</th>
                <th className="font-normal px-2.5 py-1.5 whitespace-nowrap">存储</th>
                <th className="font-normal px-2.5 py-1.5 whitespace-nowrap">运行环境</th>
                <th className="font-normal px-2.5 py-1.5 whitespace-nowrap">启动时间</th>
              </tr>
            </thead>
            <tbody>
              <tr className="align-middle">
                <td className="px-2.5 py-2 min-w-[140px]">
                  <div className="flex items-center gap-1.5 min-w-0">
                    <span
                      className={`w-1.5 h-1.5 rounded-full shrink-0 ${
                        online ? 'bg-emerald-500' : 'bg-muted-foreground/40'
                      }`}
                    />
                    <div className="min-w-0 leading-tight">
                      <div className="text-xs font-medium text-foreground truncate max-w-[180px]">
                        {runtime.instance_name || '-'}
                      </div>
                      <div className="text-[10px] font-mono text-muted-foreground truncate max-w-[180px]">
                        {runtime.instance_id || '-'}
                      </div>
                    </div>
                  </div>
                </td>
                <td className="px-2.5 py-2 whitespace-nowrap">
                  <Badge variant={online ? 'success' : 'secondary'}>
                    <span className={`w-1 h-1 rounded-full ${online ? 'bg-emerald-500' : 'bg-muted-foreground'}`} />
                    {online ? '在线' : (runtime.status || '未知')}
                  </Badge>
                </td>
                <td className="px-2.5 py-2 whitespace-nowrap">
                  <Badge variant="outline">{runtime.role || 'master'}</Badge>
                </td>
                <td className="px-2.5 py-2 whitespace-nowrap">
                  <MetricRing percent={runtime.cpu_percent} isLight={isLight} />
                </td>
                <td className="px-2.5 py-2 whitespace-nowrap">
                  <MetricRing percent={runtime.memory_percent} isLight={isLight} />
                </td>
                <td className="px-2.5 py-2 whitespace-nowrap">
                  <MetricRing percent={runtime.disk_percent} isLight={isLight} />
                </td>
                <td className="px-2.5 py-2 text-xs font-mono text-foreground whitespace-nowrap">
                  {runtime.platform || '-'}
                </td>
                <td className="px-2.5 py-2 text-xs tabular-nums text-foreground whitespace-nowrap">
                  {runtime.started_at || '-'}
                </td>
              </tr>
            </tbody>
          </table>
        </div>
      </div>
    </Card>
  );
};

const SystemAbout: React.FC = () => {
  const { themeMode } = useThemeStore();
  const isLight = themeMode === 'light';
  const [loading, setLoading] = useState(true);
  const [commits, setCommits] = useState<Commit[]>([]);
  const [current, setCurrent] = useState<Commit | null>(null);
  const [runtime, setRuntime] = useState<RuntimeInfo | null>(null);
  const [expanded, setExpanded] = useState(false);

  useEffect(() => {
    const fetchInfo = async () => {
      try {
        const res = await request.get('/system/about') as any;
        if (res?.success) {
          setCommits(res.commits || []);
          setCurrent(res.current || null);
          setRuntime(res.runtime || null);
        } else {
          message.error('获取系统信息失败');
        }
      } catch (e: any) {
        message.error(e.message || '获取系统信息失败');
      } finally {
        setLoading(false);
      }
    };
    fetchInfo();
  }, []);

  if (loading) {
    return (
      <div className="flex items-center justify-center h-[60vh]">
        <Spin size="large" />
      </div>
    );
  }

  return (
    <div className="max-w-6xl mx-auto space-y-4 pb-8 text-foreground">
      <div className="flex flex-col gap-0.5 pb-3 mb-1 border-b border-border">
        <div className="text-[13px] font-bold text-foreground leading-tight">系统关于</div>
        <div className="text-[10.5px] text-muted-foreground leading-normal">
          查看运行状态、当前版本与近期更新记录
        </div>
      </div>

      {runtime && <RuntimePanel runtime={runtime} isLight={isLight} />}

      {current && (
        <Card className="p-5">
          <div className="flex flex-col md:flex-row gap-5 items-start md:items-center justify-between">
            <div className="flex items-center gap-4">
              <div className={`w-12 h-12 rounded-lg border border-border flex items-center justify-center shrink-0 ${SITE_SURFACE}`}>
                <MonitorPlay className="w-5 h-5 text-foreground" />
              </div>
              <div>
                <div className="flex items-center gap-2 mb-1.5">
                  <h2 className="text-sm font-semibold m-0 text-foreground">系统版本</h2>
                  <Badge variant="default">LATEST</Badge>
                </div>
                <div className="flex items-center gap-2">
                  <span className="inline-flex items-center gap-1 text-sm text-muted-foreground">
                    <TagIcon className="w-3.5 h-3.5" />
                    {current.version}
                  </span>
                  <span className="rounded-md border border-border bg-muted px-2 py-0.5 text-xs font-mono text-muted-foreground">
                    {current.short_hash}
                  </span>
                </div>
              </div>
            </div>
            <div className="text-left md:text-right w-full md:w-auto">
              <div className="text-sm font-medium text-foreground mb-1.5">{current.message}</div>
              <div className="flex items-center md:justify-end gap-4 text-xs text-muted-foreground">
                <span className="inline-flex items-center gap-1.5"><User className="w-3.5 h-3.5" /> {current.author}</span>
                <span className="inline-flex items-center gap-1.5"><Calendar className="w-3.5 h-3.5" /> {current.date}</span>
              </div>
            </div>
          </div>
        </Card>
      )}

      <Card className="overflow-hidden">
        <div className="flex items-center gap-2 px-5 py-4 border-b border-border">
          <GitCommit className="w-4 h-4 text-muted-foreground shrink-0" />
          <h3 className="text-sm font-semibold m-0 text-foreground">更新记录</h3>
          <Badge variant="secondary">最近 10 次提交</Badge>
        </div>

        <div className="p-5">
          <div className="relative border-l border-border ml-3 space-y-5">
            {(expanded ? commits : commits.slice(0, 3)).map((c) => (
              <div key={c.hash} className="relative pl-6">
                <div
                  className={`absolute -left-[5px] top-3 w-2.5 h-2.5 rounded-full border-2 border-card ${
                    c.is_current ? 'bg-foreground' : 'bg-muted-foreground/40'
                  }`}
                />

                <div
                  className={`rounded-md border p-4 transition-colors ${
                    c.is_current
                      ? `border-border ${SITE_SURFACE_SOFT}`
                      : 'border-border/70 bg-card'
                  }`}
                >
                  <div className="flex flex-wrap items-center gap-2 mb-2">
                    <span className="inline-flex items-center gap-1 text-sm font-medium text-foreground">
                      <TagIcon className="w-3.5 h-3.5 text-muted-foreground" />
                      {c.version}
                    </span>
                    <span className="rounded-md border border-border bg-muted px-2 py-0.5 text-xs font-mono text-muted-foreground">
                      {c.short_hash}
                    </span>
                    {c.is_current && <Badge variant="default">当前版本</Badge>}
                  </div>

                  <p className={`text-sm mb-3 m-0 ${c.is_current ? 'text-foreground font-medium' : 'text-muted-foreground'}`}>
                    {c.message || '(无提交说明)'}
                  </p>

                  <div className="flex flex-wrap items-center gap-4 text-xs text-muted-foreground">
                    <span className="inline-flex items-center gap-1.5"><User className="w-3.5 h-3.5" /> {c.author}</span>
                    <span className="inline-flex items-center gap-1.5"><Calendar className="w-3.5 h-3.5" /> {c.date}</span>
                  </div>
                </div>
              </div>
            ))}
          </div>

          {commits.length > 3 && (
            <div className="mt-6 text-center">
              <button
                type="button"
                onClick={() => setExpanded(!expanded)}
                className="inline-flex items-center gap-1.5 h-9 px-4 rounded-md border border-border bg-background text-sm font-medium text-foreground hover:bg-muted transition-colors cursor-pointer"
              >
                {expanded ? (
                  <>收起历史记录 <ChevronUp className="w-4 h-4" /></>
                ) : (
                  <>展开更多记录 ({commits.length - 3}) <ChevronDown className="w-4 h-4" /></>
                )}
              </button>
            </div>
          )}
        </div>
      </Card>
    </div>
  );
};

export default SystemAbout;
