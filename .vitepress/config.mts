import { defineConfig } from 'vitepress'
import llmstxt, {
  copyOrDownloadAsMarkdownButtons
} from 'vitepress-plugin-llms'

export default defineConfig({
  head: [
    ['script', { async: '', src: 'https://www.googletagmanager.com/gtag/js?id=G-WE9F14MPK6' }],
    ['script', {}, `window.dataLayer = window.dataLayer || [];
function gtag(){dataLayer.push(arguments);}
gtag('js', new Date());
gtag('config', 'G-WE9F14MPK6');`]
  ],
  vite: {
    plugins: [
      {
        name: 'fix-vitepress-data-symbol',
        transform(code, id) {
          if (id.includes('vitepress/dist/client/app/data.js')) {
            return code.replace(
              'export const dataSymbol = Symbol();',
              'export const dataSymbol = "__vitepress_data__";'
            )
          }
        }
      },
      llmstxt()
    ]
  },
  srcDir: './docs',
  outDir: './.vitepress/dist',
  title: "现代 Rust 实战教程",
  description: "从语言基础到可上线的 Web API",
  ignoreDeadLinks: [
    /^https?:\/\/localhost/,
    /^https?:\/\/127\.0\.0\.1/,
    /^http:\/\/\d+\.\d+\.\d+\.\d+/,
  ],
  themeConfig: {
    socialLinks: [
      { icon: 'github', link: 'https://github.com/yuliqi/rust-web-tutorial' }
    ],
    nav: [
      { text: '首页', link: '/' },
      { text: '章节', link: '/README' },
      { text: '附录', link: '/appendix/A-common-errors' },
    ],

    sidebar: [
      {
        text: '第一篇 · 语言基础（第 0–10 章）',
        collapsed: false,
        items: [
          { text: '00 环境与工具链', link: '/chapters/00-environment' },
          { text: '01 语言基础', link: '/chapters/01-language-basics' },
          { text: '02 所有权', link: '/chapters/02-ownership' },
          { text: '03 结构体、枚举与模式匹配', link: '/chapters/03-structs-enums' },
          { text: '04 错误处理', link: '/chapters/04-error-handling' },
          { text: '05 集合与迭代器', link: '/chapters/05-collections-iterators' },
          { text: '06 泛型与 Trait', link: '/chapters/06-generics-traits' },
          { text: '07 模块与工程组织', link: '/chapters/07-modules-workspace' },
          { text: '08 测试与质量', link: '/chapters/08-testing-quality' },
          { text: '09 智能指针与内存模型', link: '/chapters/09-smart-pointers' },
          { text: '10 并发与异步', link: '/chapters/10-concurrency-async' },
        ]
      },
      {
        text: '第二篇 · Web 后端主线（第 11–13 章）',
        collapsed: false,
        items: [
          { text: '11 Web 后端生态', link: '/chapters/11-web-ecosystem' },
          { text: '12 综合项目：Todo API', link: '/chapters/12-todo-api-project' },
          { text: '13 进阶路线图', link: '/chapters/13-advanced-roadmap' },
        ]
      },
      {
        text: '第三篇 · 生产基建与部署（第 14–16 章）',
        collapsed: false,
        items: [
          { text: '14 生产中间件实战', link: '/chapters/14-middleware-production' },
          { text: '15 集群与分布式入门', link: '/chapters/15-cluster-distributed' },
          { text: '16 部署与上线', link: '/chapters/16-deployment' },
        ]
      },
      {
        text: '第四篇 · SaaS 业务与安全（第 17–20 章）',
        collapsed: false,
        items: [
          { text: '17 SaaS：多租户与鉴权', link: '/chapters/17-multitenancy-auth' },
          { text: '18 SaaS：配额、计费与 API 治理', link: '/chapters/18-quota-billing-api' },
          { text: '19 接口报文加密', link: '/chapters/19-api-encryption' },
          { text: '20 数据库敏感数据存储', link: '/chapters/20-data-at-rest' },
        ]
      },
      {
        text: '第五篇 · 平台化进阶（第 21–29 章）',
        collapsed: false,
        items: [
          { text: '21 定时任务与后台作业', link: '/chapters/21-scheduled-jobs' },
          { text: '22 多云资产同步引擎', link: '/chapters/22-cloud-asset-sync' },
          { text: '23 两步验证与层级账号（IAM）', link: '/chapters/23-2fa-iam' },
          { text: '24 实时通信：WebSocket 与 SSE', link: '/chapters/24-realtime-websocket' },
          { text: '25 软件许可：离线签名与在线激活', link: '/chapters/25-software-license' },
          { text: '26 Web 终端与堡垒机', link: '/chapters/26-web-terminal-bastion' },
          { text: '27 证书管理：ACME 自动签发续期', link: '/chapters/27-certificate-acme' },
          { text: '28 实时监控与告警', link: '/chapters/28-realtime-monitoring' },
          { text: '29 应用商店与插件机制', link: '/chapters/29-appstore-plugins' },
        ]
      },
      {
        text: '附录',
        collapsed: false,
        items: [
          { text: 'A. 常见编译错误速查', link: '/appendix/A-common-errors' },
          { text: 'B. 与其他语言对照', link: '/appendix/B-language-comparison' },
          { text: 'C. 面试高频题', link: '/appendix/C-interview' },
          { text: 'D. 资源清单', link: '/appendix/D-resources' },
          { text: 'E. 1.97.1 工具链速查', link: '/appendix/E-toolchain-2026' },
          { text: 'F. 1.97 新特性清单与映射', link: '/appendix/F-rust-197-features' },
          { text: 'G. 配置与命名约定', link: '/appendix/G-conventions' },
          { text: 'H. 堡垒机架构与选型指引', link: '/appendix/H-bastion-architecture' },
        ]
      }
    ],
    search: {
      provider: 'local',
      options: {
        detailedView: true,
        translations: {
          button: {
            buttonText: '搜索',
            buttonAriaLabel: '搜索文档'
          },
          modal: {
            displayDetails: '显示详情',
            resetButtonTitle: '清除搜索',
            backButtonTitle: '返回',
            noResultsText: '未找到相关结果',
            footer: {
              selectText: '选择',
              navigateText: '切换',
              closeText: '关闭'
            }
          }
        }
      }
    }
  },
  markdown: {
    config(md) {
      md.use(copyOrDownloadAsMarkdownButtons)
    }
  },
  
})
