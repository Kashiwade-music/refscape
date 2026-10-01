import { Pressable, Text } from 'react-native';
import type { CounterProps } from '../../src/CounterView';

export function CounterView({ counter, onIncrement }: CounterProps) {
  return <Pressable onPress={onIncrement}><Text>{counter.value}</Text></Pressable>;
}
