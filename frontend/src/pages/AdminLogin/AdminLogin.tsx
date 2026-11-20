/*
 * tokensbyte opensource
 * (c) 2026 tokensbyte.ai
 * @copyright      Copyright netbcloud/wstianxia 
 * @license        MIT (https://www.tokensbyte.ai/)
 */

import React, { useState, useEffect } from 'react';
import { Form, Input, Button, Typography, message, ConfigProvider, Card, theme as antdTheme } from 'antd';
import { useNavigate } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import axios, { AxiosError } from 'axios';
import useAuthStore from '../../store/auth';
import type { User } from '../../types';
import { clearAwaitingFreshSetup, fetchAdminInitStatus, isAwaitingFreshSetup } from '../../utils/freshSetup';

const { Title, Text } = Typography;

const BRAND = 'tokens-box';
const PRIMARY = '#165cff';
const TEXT_MAIN = '#1f2024';
const TEXT_SUB = '#86909c';

const pageBackground = [
  'radial-gradient(900px 520px at 12% 8%, #dbe6ff 0%, rgba(219,230,255,0) 62%)',
  'radial-gradient(760px 480px at 88% 14%, #e9e0ff 0%, rgba(233,224,255,0) 60%)',
  'radial-gradient(680px 520px at 62% 96%, #d9ecff 0%, rgba(217,236,255,0) 58%)',
  'linear-gradient(135deg, #eef2fb 0%, #f8fafd 48%, #eaf0fb 100%)',
].join(', ');

const captionStyle: React.CSSProperties = { color: TEXT_SUB, fontSize: 14, marginRight: 4 };
const filledInputStyle: React.CSSProperties = { borderRadius: 6 };
const submitButtonStyle: React.CSSProperties = {
  height: 46,
  borderRadius: 6,
  border: 'none',
  fontWeight: 600,
  fontSize: 15,
  boxShadow: '0 6px 16px rgba(22,92,255,.24)',
};

interface LoginResponse {
  token: string;
  user: User;
}

const AdminLogin: React.FC = () => {
  const { t } = useTranslation();
  const [loading, setLoading] = useState(false);
  const [isInitialized, setIsInitialized] = useState<boolean | null>(null);
  const [waitingRestart, setWaitingRestart] = useState(isAwaitingFreshSetup());

  const navigate = useNavigate();
  const { token, user, setToken, setUser } = useAuthStore();

  // 检查系统是否有管理员；清空库后服务重启期间持续探测，直到返回明确状态
  useEffect(() => {
    let cancelled = false;
    const checkInitStatus = async () => {
      while (!cancelled) {
        try {
          const initialized = await fetchAdminInitStatus();
          if (cancelled) return;
          clearAwaitingFreshSetup();
          setWaitingRestart(false);
          setIsInitialized(initialized);
          return;
        } catch {
          if (cancelled) return;
          setWaitingRestart(true);
          await new Promise((resolve) => window.setTimeout(resolve, 1000));
        }
      }
    };
    void checkInitStatus();
    return () => {
      cancelled = true;
    };
  }, []);

  // 如果管理员已登录，且系统已初始化，直接跳转到控制台页面
  useEffect(() => {
    if (isInitialized && token && user?.role === 'admin') {
      const adminPath = localStorage.getItem('tokensbyte_admin_path') || 'admin1688';
      navigate(`/${adminPath}/dashboard`, { replace: true });
    }
  }, [isInitialized, token, user, navigate]);

  // 正常管理员登录
  const onLoginFinish = async (values: unknown) => {
    setLoading(true);
    try {
      const response = await axios.post<LoginResponse>('/api/v1/auth/admin/login', values);
      const { token, user } = response.data;
      
      if (user.role !== 'admin') {
        message.error('该账号不是管理员，无法登录后台');
        return;
      }

      setToken(token);
      setUser(user);
      message.success(t('auth.admin_success') || '登录成功');
      const adminPath = localStorage.getItem('tokensbyte_admin_path') || 'admin1688';
      navigate(`/${adminPath}/dashboard`);
    } catch (error) {
      console.error(error);
      const axiosError = error as AxiosError<{ error: { message: string } }>;
      message.error(axiosError.response?.data?.error?.message || '登录失败，请稍后重试');
    } finally {
      setLoading(false);
    }
  };

  // 首次初始化创建超级管理员
  const onInitFinish = async (values: any) => {
    if (values.password !== values.confirmPassword) {
      message.error('两次输入的密码不一致');
      return;
    }
    setLoading(true);
    try {
      const response = await axios.post<LoginResponse>('/api/v1/auth/admin/init', {
        username: values.username,
        password: values.password,
        timezone: Intl.DateTimeFormat().resolvedOptions().timeZone,
      });
      const { token, user } = response.data;
      setToken(token);
      setUser(user);
      message.success('超级管理员初始化成功！欢迎登录。');
      setIsInitialized(true);
      const adminPath = localStorage.getItem('tokensbyte_admin_path') || 'admin1688';
      // 整页跳转，让 AdminSetupGate 重新探测 initialized 状态
      window.location.assign(`/${adminPath}/dashboard`);
    } catch (error) {
      console.error(error);
      const axiosError = error as AxiosError<{ error: { message: string } }>;
      message.error(axiosError.response?.data?.error?.message || '初始化失败，请稍后重试');
    } finally {
      setLoading(false);
    }
  };

  if (isInitialized === null) {
    return (
      <div style={{
        minHeight: '100vh',
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'center',
        justifyContent: 'center',
        background: pageBackground,
        color: '#4e5969',
        fontSize: 14,
      }}>
        <div>正在检查系统状态…</div>
        {waitingRestart && (
          <div style={{ marginTop: 12, fontSize: 12, color: TEXT_SUB, textAlign: 'center', lineHeight: 1.7 }}>
            <div>服务正在重启，随后将进入全新安装</div>
          </div>
        )}
      </div>
    );
  }

  return (
    <ConfigProvider
      theme={{
        algorithm: antdTheme.defaultAlgorithm,
        token: {
          colorPrimary: PRIMARY,
          colorText: TEXT_MAIN,
          colorTextPlaceholder: TEXT_SUB,
          colorBgContainer: '#ffffff',
          colorFillAlter: '#f2f3f5',
          colorBorder: '#e5e6eb',
          borderRadius: 6,
        },
        components: {
          Button: {
            colorPrimary: PRIMARY,
            colorPrimaryHover: '#0e4cd9',
            colorPrimaryActive: '#0b40b8',
          },
        },
      }}
    >
      <div
        className="admin-login-page"
        style={{
        minHeight: '100vh',
        display: 'flex',
        flexDirection: 'column',
        background: pageBackground,
        position: 'relative',
        overflow: 'hidden',
      }}>
        {/* 斜向高光 */}
        <div style={{
          position: 'absolute',
          top: 0,
          left: 0,
          width: '100%',
          height: '100%',
          pointerEvents: 'none',
          opacity: 0.55,
          background: [
            'linear-gradient(112deg, rgba(255,255,255,0) 42%, rgba(255,255,255,.9) 48%, rgba(255,255,255,0) 54%)',
            'linear-gradient(78deg, rgba(255,255,255,0) 66%, rgba(255,255,255,.7) 71%, rgba(255,255,255,0) 76%)',
          ].join(', '),
        }} />

        {/* 品牌区 */}
        <div style={{ display: 'flex', alignItems: 'center', padding: '18px 28px', position: 'relative', zIndex: 2 }}>
          <div style={{
            width: 30,
            height: 30,
            borderRadius: 7,
            background: 'linear-gradient(135deg, #165cff 0%, #4b8bff 100%)',
            color: '#fff',
            fontWeight: 800,
            fontSize: 14,
            letterSpacing: '.5px',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            boxShadow: '0 4px 12px rgba(22,92,255,.28)',
          }}>
            TB
          </div>
          <div style={{ marginLeft: 9, fontSize: 17, fontWeight: 700, color: TEXT_MAIN, letterSpacing: '.2px' }}>
            {BRAND}
          </div>
        </div>

        <div style={{
          flex: 1,
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          padding: '0 20px 40px',
          position: 'relative',
          zIndex: 2,
        }}>
          <Card
            style={{
              width: 560,
              maxWidth: '100%',
              borderRadius: 10,
              border: 'none',
              boxShadow: '0 18px 48px rgba(31,56,88,.13), 0 2px 6px rgba(31,56,88,.06)',
            }}
            styles={{ body: { padding: '30px 40px 32px' } }}
          >
            <Title level={3} style={{ margin: 0, marginBottom: 20, fontSize: 22, fontWeight: 700 }}>
              登录
            </Title>

            {isInitialized ? (
              /* 正常管理员登录表单 */
              <Form
                name="admin_auth"
                size="large"
                onFinish={onLoginFinish}
                autoComplete="off"
                layout="vertical"
              >
                <Form.Item
                  name="username"
                  rules={[{ required: true, message: '请输入账号名' }]}
                >
                  <Input
                    variant="filled"
                    prefix={<span style={captionStyle}>账号名</span>}
                    placeholder="请输入"
                    style={filledInputStyle}
                  />
                </Form.Item>

                <Form.Item
                  name="password"
                  rules={[{ required: true, message: '请输入密码' }]}
                >
                  <Input.Password
                    variant="filled"
                    prefix={<span style={captionStyle}>密码</span>}
                    placeholder="请输入"
                    style={filledInputStyle}
                  />
                </Form.Item>

                <Form.Item style={{ marginTop: 8, marginBottom: 0 }}>
                  <Button
                    type="primary"
                    htmlType="submit"
                    block
                    loading={loading}
                    style={submitButtonStyle}
                  >
                    立即登录
                  </Button>
                </Form.Item>
              </Form>
            ) : (
              /* 首次初始化超级管理员表单 */
              <>
                <Text style={{ color: TEXT_SUB, fontSize: 13, display: 'block', marginBottom: 18 }}>
                  [检测到系统未设置管理员，请创建首个超级管理员]
                </Text>
                <Form
                  name="admin_init"
                  size="large"
                  onFinish={onInitFinish}
                  autoComplete="off"
                  layout="vertical"
                >
                  <Form.Item
                    name="username"
                    rules={[{ required: true, message: '请输入管理员用户名' }]}
                  >
                    <Input
                      variant="filled"
                      prefix={<span style={captionStyle}>账号名</span>}
                      placeholder="请输入"
                      style={filledInputStyle}
                    />
                  </Form.Item>

                  <Form.Item
                    name="password"
                    rules={[
                      { required: true, message: '请输入管理员密码' },
                      { min: 6, message: '密码长度至少为 6 位' }
                    ]}
                  >
                    <Input.Password
                      variant="filled"
                      prefix={<span style={captionStyle}>密码</span>}
                      placeholder="请输入"
                      style={filledInputStyle}
                    />
                  </Form.Item>

                  <Form.Item
                    name="confirmPassword"
                    dependencies={['password']}
                    rules={[
                      { required: true, message: '请确认管理员密码' },
                      ({ getFieldValue }) => ({
                        validator(_, value) {
                          if (!value || getFieldValue('password') === value) {
                            return Promise.resolve();
                          }
                          return Promise.reject(new Error('两次输入的密码不一致'));
                        },
                      }),
                    ]}
                  >
                    <Input.Password
                      variant="filled"
                      prefix={<span style={captionStyle}>确认密码</span>}
                      placeholder="请再次输入"
                      style={filledInputStyle}
                    />
                  </Form.Item>

                  <Form.Item style={{ marginTop: 8, marginBottom: 0 }}>
                    <Button
                      type="primary"
                      htmlType="submit"
                      block
                      loading={loading}
                      style={submitButtonStyle}
                    >
                      创建并登录
                    </Button>
                  </Form.Item>
                </Form>
              </>
            )}
          </Card>
        </div>

        {/* 全局暗色主题把主按钮文字钉成深色（index.css），本页蓝色主按钮需要反回来 */}
        <style>
          {`
            body[data-theme='dark'] .admin-login-page .ant-btn-primary:not(:disabled):not(.ant-btn-dangerous),
            body[data-theme='dark'] .admin-login-page .ant-btn-primary:not(:disabled):not(.ant-btn-dangerous):hover {
              color: #ffffff !important;
            }
          `}
        </style>
      </div>
    </ConfigProvider>
  );
};

export default AdminLogin;
