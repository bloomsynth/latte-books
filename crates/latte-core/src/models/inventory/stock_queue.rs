// Port of models/inventory/stockQueue.ts @ v0.36.0: the stock of an item at a location,
// as the rates it came in at, oldest first.

#[derive(Debug, Clone, Default)]
pub struct StockQueue {
    pub quantity: f64,
    pub value: f64,
    pub queue: Vec<(f64, f64)>,
    pub moving_average: f64,
}

impl StockQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// `fifo`: the valuation rate, as the stock's value over its quantity.
    pub fn fifo(&self) -> f64 {
        let valuation = self.value / self.quantity;
        if valuation.is_nan() { 0.0 } else { valuation }
    }

    /// `inward`: stock coming in. Returns the rate, or `None` for a quantity that isn't
    /// positive or a negative rate.
    pub fn inward(&mut self, rate: f64, quantity: f64) -> Option<f64> {
        if quantity <= 0.0 || rate < 0.0 {
            return None;
        }

        let inward_value = rate * quantity;
        self.moving_average =
            (self.moving_average * self.quantity + inward_value) / (self.quantity + quantity);
        self.quantity += quantity;
        self.value += inward_value;

        match self.queue.last_mut() {
            Some((last_rate, last_quantity)) if *last_rate == rate => *last_quantity += quantity,
            _ => self.queue.push((rate, quantity)),
        }
        Some(rate)
    }

    /// `outward`: stock going out, oldest first. Returns the rate it went out at, or
    /// `None` if there isn't that much.
    pub fn outward(&mut self, quantity: f64) -> Option<f64> {
        if self.quantity < quantity || quantity <= 0.0 {
            return None;
        }

        let mut incoming_rate = 0.0;
        self.quantity -= quantity;
        let mut remaining = quantity;
        while remaining > 0.0 {
            if self.queue.is_empty() {
                return None;
            }
            let (rate, stored_quantity) = self.queue.remove(0);
            let mut quantity_removed = remaining;
            let quantity_left = stored_quantity - remaining;
            remaining -= stored_quantity;
            if remaining > 0.0 {
                quantity_removed = stored_quantity;
            }
            if quantity_left > 0.0 {
                self.queue.insert(0, (rate, quantity_left));
            }
            self.value -= rate * quantity_removed;
            incoming_rate += quantity_removed * rate;
        }
        Some(incoming_rate / quantity)
    }
}

#[cfg(test)]
mod tests {
    use super::StockQueue;

    // testStockQueue.spec.ts.
    #[test]
    fn values_stock_first_in_first_out() {
        let mut q = StockQueue::new();
        assert_eq!(q.inward(100.0, 10.0), Some(100.0));
        assert_eq!(q.inward(200.0, 10.0), Some(200.0));
        assert_eq!(q.fifo(), 150.0);
        assert_eq!(q.outward(15.0), Some((10.0 * 100.0 + 5.0 * 200.0) / 15.0));
        assert_eq!(q.quantity, 5.0);
        assert_eq!(q.value, 1000.0);
        assert_eq!(q.outward(6.0), None);
        assert_eq!(q.inward(-1.0, 1.0), None);
        assert_eq!(q.outward(5.0), Some(200.0));
        assert_eq!(q.fifo(), 0.0);
    }
}
