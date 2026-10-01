import { CounterView } from './CounterView';

export function NativeLegacy() {
  return <CounterView counter={{ value: 2 }} onIncrement={() => {}} />;
}
