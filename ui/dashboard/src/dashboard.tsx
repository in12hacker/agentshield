import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from 'react';
import { Activity, Cpu, FileText,
  LayoutDashboard, Moon, Settings, Shield, ShieldCheck, Sun, Zap,
  Menu, MessagesSquare, GitBranch, type LucideIcon } from 'lucide-react';
import { driverFacts, featureView, flowFacts, flowView, overviewView } from './overview-model';
import type { OverviewRead } from './reply-wire';
import { LanguageSelect, useI18n } from './i18n';
import { dashboardPages, navigationGroups, type DashboardLocation, type DashboardPage, type DashboardPageId } from './global/navigation';

const navigationIcons: Record<DashboardPageId, LucideIcon> = {
  overview: LayoutDashboard, processes: Cpu, rules: FileText, activity: Activity,
  group: GitBranch, chat: MessagesSquare, settings: Settings, egress: Shield,
};
function NavItem({ page, active, navigate }: {
  page: DashboardPage; active: DashboardPageId; navigate: () => void;
}) {
  const { t } = useI18n();
  const Icon = navigationIcons[page.id];
  const text = t(page.titleKey);
  const selected = page.id === active;
  return <a className={`nav-item ${selected ? 'selected' : ''}`} href={page.href} onClick={event => {
    if (!event.defaultPrevented && event.button === 0 && !event.metaKey && !event.ctrlKey && !event.shiftKey && !event.altKey) navigate();
  }}
    aria-current={selected ? 'page' : undefined}>
    <Icon size={18} aria-hidden="true" /><span>{text}</span>
  </a>;
}
export function DashboardShell({ route, askVisible, read, refresh, lock, children, notice, overlay }: {
  route: DashboardLocation; askVisible: boolean; read: OverviewRead; refresh: () => void; lock: () => void;
  children: ReactNode; notice?: ReactNode; overlay?: ReactNode;
}) {
  const { t } = useI18n();
  const [dark, setDark] = useState(() => document.documentElement.classList.contains('dark')
    || (!document.documentElement.classList.contains('light') && window.matchMedia('(prefers-color-scheme: dark)').matches));
  useEffect(() => {
    document.documentElement.classList.toggle('dark', dark);
    document.documentElement.classList.toggle('light', !dark);
  }, [dark]);
  const [narrow, setNarrow] = useState(() => window.matchMedia('(max-width: 767px)').matches);
  const [menuOpen, setMenuOpen] = useState(false);
  const menuButton = useRef<HTMLButtonElement>(null);
  const navigation = useRef<HTMLElement>(null);
  const main = useRef<HTMLElement>(null);
  const pendingFocus = useRef(true);
  const [repeatNavigation, setRepeatNavigation] = useState(0);
  useEffect(() => {
    const media = window.matchMedia('(max-width: 767px)');
    const resize = () => {
      setNarrow(media.matches);
      setMenuOpen(false);
      if (media.matches && navigation.current?.contains(document.activeElement)
        && !document.querySelector('dialog[open]')) menuButton.current?.focus();
    };
    media.addEventListener('change', resize);
    return () => media.removeEventListener('change', resize);
  }, []);
  useLayoutEffect(() => {
    setMenuOpen(false);
    pendingFocus.current = true;
    const element = main.current;
    if (!element) return;
    // Only navigation moves the scroll position, never an asynchronous receipt.
    const section = route.page.href !== route.target ? document.getElementById(route.target.slice(1)) : null;
    element.scrollTop = section ? element.scrollTop + section.getBoundingClientRect().top - element.getBoundingClientRect().top : 0;
  }, [route.hash, repeatNavigation]);
  useLayoutEffect(() => {
    let frame = 0;
    const focus = () => {
      if (!pendingFocus.current || askVisible || document.querySelector('dialog[open]')) return;
      const heading = document.getElementById(route.headingId);
      heading?.focus({ preventScroll: true });
      pendingFocus.current = false;
    };
    const schedule = () => { cancelAnimationFrame(frame); frame = requestAnimationFrame(focus); };
    schedule();
    document.addEventListener('close', schedule, true);
    return () => { cancelAnimationFrame(frame); document.removeEventListener('close', schedule, true); };
  }, [route.hash, repeatNavigation, askVisible]);
  useLayoutEffect(() => {
    if (!menuOpen || !narrow || askVisible || document.querySelector('dialog[open]')) return;
    navigation.current?.querySelector<HTMLElement>('[aria-current="page"]')?.focus();
  }, [menuOpen, narrow, askVisible]);
  useEffect(() => {
    if (!menuOpen || !narrow) return;
    const escape = (event: KeyboardEvent) => {
      if (event.key !== 'Escape' || event.defaultPrevented || document.querySelector('dialog[open]')) return;
      event.preventDefault();
      setMenuOpen(false);
      menuButton.current?.focus();
    };
    window.addEventListener('keydown', escape);
    return () => window.removeEventListener('keydown', escape);
  }, [menuOpen, narrow]);
  const driver = driverFacts(read);
  const flow = flowFacts(read);
  return <div className="dashboard-shell">
    <a className="skip-link" href={`#${route.page.headingId}`} onClick={event => {
      event.preventDefault();
      document.getElementById(route.page.headingId)?.focus();
    }}>{t('nav.skipToContent')}</a>
    <aside id="dashboard-navigation" className="dashboard-sidebar" hidden={narrow && !menuOpen}>
      <div className="brand"><div className="brand-mark"><Shield /></div><div className="brand-copy">
        <strong>{t('app.brand')}</strong><p><i className="connection-dot" />{t('app.tagline')}</p>
      </div></div>
      <nav ref={navigation} aria-label={t('nav.main')}>
        {navigationGroups.map(group => <section className="nav-group" key={group.id} aria-labelledby={`nav-${group.id}`}>
          <h2 id={`nav-${group.id}`}>{t(group.titleKey)}</h2>
          {dashboardPages.filter(page => page.group === group.id).map(page => <NavItem key={page.id} page={page} active={route.page.id}
            navigate={() => {
              setMenuOpen(false);
              if (page.href === route.hash) setRepeatNavigation(value => value + 1);
            }} />)}
        </section>)}
        <div className="sidebar-guards"><h3>{t('sidebar.active')}</h3>
          <div><span><Cpu size={12} />{t('sidebar.process')}</span><span>{t(featureView(driver?.process?.effective_state).label)}</span></div>
          <div><span><Zap size={12} />{t('sidebar.proxy')}</span><span>{t(flowView(flow?.intake_state).label)}</span></div>
          <p>{t('sidebar.note')}</p>
        </div>
      </nav>
    </aside>
    <div className="dashboard-column">
      <header className="dashboard-header">
        <button ref={menuButton} type="button" className="menu-button" aria-expanded={menuOpen}
          aria-controls="dashboard-navigation" onClick={() => setMenuOpen(value => !value)}>
          <Menu size={18} aria-hidden="true" /><span>{t('nav.menu')}</span>
        </button>
        <div className="header-stats">
          <div className="header-stat">
            <div className="header-stat-icon"><ShieldCheck size={18} /></div>
            <div><span>{t('header.protection')}</span><strong>{t(overviewView(read).shortLabel)}</strong></div>
          </div>
        </div>
        <div className="header-tools">
          <button type="button" className="lock-button" onClick={lock} aria-label={t('header.lock')}>{t('header.lockShort')}</button>
          <button type="button" className="icon-button" onClick={refresh} title={t('header.refresh')} aria-label={t('header.refresh')}><Activity size={18} /></button>
          <LanguageSelect />
          <button type="button" className="icon-button" onClick={() => setDark(value => !value)} aria-pressed={dark} title={t('header.theme')} aria-label={t('header.theme')}>{dark ? <Sun size={18} /> : <Moon size={18} />}</button>
        </div>
      </header>
      <main ref={main} className="dashboard-main" aria-labelledby={route.page.headingId}>
        <div className="workspace-page">
          <div className="page-title"><h1 id={route.page.headingId} tabIndex={-1}>{t(route.page.titleKey)}</h1></div>
          {children}
        </div>
      </main>
      {notice}
    </div>
    {overlay}
  </div>;
}
