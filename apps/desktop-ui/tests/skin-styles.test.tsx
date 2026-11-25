import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import { Button, Progress, Card } from 'antd';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

describe('Skin Styles & Micro-Interactions (styles.css verification)', () => {
  const css = readFileSync(resolve(__dirname, '../src/styles.css'), 'utf-8');

  it('contains Win98 classic 3D double-bevel and sunken box-shadow styles', () => {
    expect(css).toContain("[data-skin='win98'] .telemetry-card");
    expect(css).toContain('inset 1px 1px 0px #ffffff, inset -1px -1px 0px #808080');
    expect(css).toContain("[data-skin='win98'] .ant-btn:active");
    expect(css).toContain('transform: translate(1px, 1px)');
    expect(css).toContain("[data-skin='win98'] .ant-progress-bg");
    expect(css).toContain('repeating-linear-gradient');
  });

  it('contains Win11 Fluent mica backdrop-filter and specular top highlight', () => {
    expect(css).toContain("[data-skin='win11'] .header-bar");
    expect(css).toContain('backdrop-filter: blur(20px) saturate(180%)');
    expect(css).toContain("[data-skin='win11'] .telemetry-card");
    expect(css).toContain('inset 0 1px 0 0 rgba(255, 255, 255, 0.7)');
    expect(css).toContain("[data-skin='win11'] .ant-btn:active");
    expect(css).toContain('scale(0.985)');
  });

  it('contains Cyberpunk neon grid and electric glow styles', () => {
    expect(css).toContain("[data-skin='cyberpunk'] .page-content");
    expect(css).toContain('linear-gradient(rgba(0, 240, 255, 0.04) 1px, transparent 1px)');
    expect(css).toContain("[data-skin='cyberpunk'] .ant-btn-primary");
    expect(css).toContain('box-shadow: 0 0 10px rgba(0, 240, 255, 0.5)');
    expect(css).toContain('@keyframes ou-cyber-beam');
    expect(css).toContain('animation: ou-cyber-beam');
  });

  it('contains Modern Minimal clean hairline borders and subtle elevation', () => {
    expect(css).toContain("[data-skin='modern'] .telemetry-card");
    expect(css).toContain('border: 1px solid rgba(0, 0, 0, 0.08)');
    expect(css).toContain("[data-skin='modern'] .ant-btn:active");
    expect(css).toContain('transform: translateY(1px)');
  });

  it('contains universal progress streamer and breathing feedback animations', () => {
    expect(css).toContain('.ant-progress-bg::after');
    expect(css).toContain('@keyframes ou-streamer');
    expect(css).toContain('animation: ou-streamer');
    expect(css).toContain('.ou-breathe');
    expect(css).toContain('@keyframes ou-breathe');
  });

  it('safely disables motion for Win98 and reduced-motion environments', () => {
    expect(css).toContain('@media (prefers-reduced-motion: reduce)');
    expect(css).toContain("[data-skin='win98'] .ou-breathe");
    expect(css).toContain('animation: none !important');
  });

  it('renders UI elements inside skin containers without DOM errors', () => {
    const { container } = render(
      <div data-skin="cyberpunk" data-theme="dark">
        <Button type="primary">Cyber Action</Button>
        <Progress percent={60} />
        <Card title="Telemetry">Metrics</Card>
      </div>,
    );
    expect(screen.getByText('Cyber Action')).toBeTruthy();
    expect(screen.getByText('Telemetry')).toBeTruthy();
    expect(container.querySelector('[data-skin="cyberpunk"]')).toBeTruthy();
  });
});
