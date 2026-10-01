export interface Counter {
  value: number;
}

export namespace Counters {
  export class Controller {
    reset(counter: Counter): Counter {
      return { value: 0 };
    }

    increment(counter: Counter): Counter {
      return { value: counter.value + 1 };
    }
  }
}

export function initialCounter(): Counter {
  return { value: 0 };
}
