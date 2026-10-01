import { CounterView } from './CounterView';

export function Legacy() {
  return <CounterView counter={{ value: 1 }} onIncrement={() => {}} />;
}
