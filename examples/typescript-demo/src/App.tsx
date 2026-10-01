import { useState } from 'react';
import { CounterView } from './CounterView';
import { Counters, initialCounter } from '@demo/model';

export default function App() {
  const [counter, setCounter] = useState(initialCounter);
  const controller = new Counters.Controller();
  return <CounterView counter={counter} onIncrement={() => setCounter(controller.increment(counter))} />;
}
