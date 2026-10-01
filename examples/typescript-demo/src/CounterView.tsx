import type { Counter } from '@demo/model';

export interface CounterProps {
  counter: Counter;
  onIncrement: () => void;
}

export function CounterView({ counter, onIncrement }: CounterProps) {
  return <button onClick={onIncrement}>{counter.value}</button>;
}
