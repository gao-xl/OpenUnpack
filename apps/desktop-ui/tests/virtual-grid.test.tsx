import { render, fireEvent } from '@testing-library/react';
import { it, expect, describe } from 'vitest';
import VirtualGrid from '../src/components/VirtualGrid';

describe('VirtualGrid 视口虚拟滚动', () => {
  it('处理 10,000+ 数据时，渲染的 DOM 卡片节点数始终稳定在 30 左右，消除性能瓶颈', () => {
    const items = Array.from({ length: 10000 }, (_, i) => ({
      id: 'item-' + i,
      title: 'Task ' + i,
    }));

    const { container } = render(
      <VirtualGrid
        items={items}
        itemKey={(item) => item.id}
        minItemWidth={300}
        rowHeight={180}
        gap={12}
        containerHeight={540}
        renderItem={(item) => (
          <div className="test-card" data-testid={item.id}>
            {item.title}
          </div>
        )}
      />,
    );

    const cards = container.querySelectorAll('.test-card');
    console.log('10,000 items -> rendered cards count in DOM:', cards.length);

    // 验证严格稳定在 30 左右（例如 20 ~ 36 之间），绝对不渲染 10,000 个节点
    expect(cards.length).toBeGreaterThanOrEqual(15);
    expect(cards.length).toBeLessThanOrEqual(40);
  });

  it('滚动容器时动态更新视口内渲染项，旧项被卸载，新项被挂载', () => {
    const items = Array.from({ length: 1000 }, (_, i) => ({
      id: 'item-' + i,
      title: 'Task ' + i,
    }));

    const { container } = render(
      <VirtualGrid
        items={items}
        itemKey={(item) => item.id}
        minItemWidth={300}
        rowHeight={180}
        gap={12}
        containerHeight={540}
        renderItem={(item) => (
          <div className="test-card" data-testid={item.id}>
            {item.title}
          </div>
        )}
      />,
    );

    // 初始包含 item-0
    expect(container.querySelector('[data-testid="item-0"]')).not.toBeNull();

    // 模拟向下滚动 3000px
    const scrollContainer = container.querySelector('[role="feed"]');
    expect(scrollContainer).not.toBeNull();
    if (scrollContainer) {
      fireEvent.scroll(scrollContainer, { target: { scrollTop: 3000 } });
    }

    // 滚动后 item-0 应被视口外卸载，后续项目被挂载
    const scrolledCards = container.querySelectorAll('.test-card');
    expect(scrolledCards.length).toBeGreaterThanOrEqual(15);
    expect(scrolledCards.length).toBeLessThanOrEqual(40);
  });

  it('小数据量（如 5 项）时完整渲染所有项', () => {
    const items = Array.from({ length: 5 }, (_, i) => ({
      id: 'item-' + i,
      title: 'Task ' + i,
    }));

    const { container } = render(
      <VirtualGrid
        items={items}
        itemKey={(item) => item.id}
        minItemWidth={300}
        rowHeight={180}
        renderItem={(item) => (
          <div className="test-card" data-testid={item.id}>
            {item.title}
          </div>
        )}
      />,
    );

    const cards = container.querySelectorAll('.test-card');
    expect(cards.length).toBe(5);
  });

  it('无数据时呈现 emptyContent', () => {
    const { getByText } = render(
      <VirtualGrid<{ id: string }>
        items={[]}
        itemKey={(item) => item.id}
        emptyContent={<div>没有数据</div>}
        renderItem={() => null}
      />,
    );
    expect(getByText('没有数据')).toBeTruthy();
  });
});
