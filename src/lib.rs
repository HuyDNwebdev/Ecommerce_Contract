use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::LookupMap;
use near_sdk::json_types::U128;
use near_sdk::serde::{Deserialize, Serialize};
use near_sdk::{
    env, ext_contract, near_bindgen, AccountId, Balance, BorshStorageKey, Gas, PanicOnDefault,
    Promise, PromiseOrValue, PromiseResult,
};

mod order;
use order::*;

pub type OrderId = String;
pub const TRANSFER_GAS: Gas = Gas(10_000_000_000_000);

#[derive(BorshDeserialize, BorshSerialize, PanicOnDefault)]
#[near_bindgen]
struct EcommerceContract {
    pub owner_id: AccountId,
    pub orders: LookupMap<OrderId, Order>,
}

#[derive(BorshDeserialize, BorshSerialize, BorshStorageKey)]
enum StorageKey {
    OrderKey,
}

#[ext_contract(ext_self)]
pub trait ExtEcommerceContract {
    fn transfer_callback(&mut self, order_id: OrderId) -> PromiseOrValue<U128>;
}
#[near_bindgen]
impl EcommerceContract {
    #[init]
    pub fn new(owner_id: AccountId) -> Self {
        Self {
            owner_id,
            orders: LookupMap::new(StorageKey::OrderKey),
        }
    }
    #[payable] //cho phep user nap tien vao
    pub fn pay_order(&mut self, order_id: OrderId, order_amount: U128) -> PromiseOrValue<U128> {
        // Lay thong tin so NEAR deposit cua user env::attached_deposit()
        assert!(
            env::attached_deposit() >= order_amount.0,
            "ERROR_DEPOSIT_NOT_ENOUGH"
        );

        // Luu tru thong tin thanh toan cua user
        let order: Order = Order {
            order_id: order_id.clone(),
            payer_id: env::signer_account_id(),
            amount: order_amount.0,
            received_amount: env::attached_deposit(),
            is_completed: true,
            is_refund: false,
            created_at: env::block_timestamp(),
        };

        self.orders.insert(&order_id, &order);

        // Tra lai tien thua cho user
        if env::attached_deposit() > order_amount.0 {
            let promise: Promise = Promise::new(env::signer_account_id())
                .transfer(env::attached_deposit() - order_amount.0);
            return PromiseOrValue::Value(U128(env::attached_deposit() - order_amount.0));
        }
        PromiseOrValue::Value(U128(0))
    }

    // Trả lại data cho user thong qua DTOs -> Data Transfer Object
    pub fn get_order(&self, order_id: OrderId) -> Order {
        self.orders.get(&order_id).expect("NOT_FOUND_ORDER_ID")
    }

    // Refund lai tien cho user
    /**
     * Kiem tra xem nguoi goi co phai la owner cuar contract khong?
     * Kiem xem don hang da complete va refund chua?
     * Thuc hien viec cap nhat trang thai don + tra tien cho user
     */

    pub fn refund(&mut self, order_id: OrderId) -> PromiseOrValue<U128> {
        //Kiem tra xem nguoi goi co phai la owner cuar contract khong?
        assert_eq!(env::predecessor_account_id(), self.owner_id);

        // get order dang muon refund
        let mut order = self.orders.get(&order_id).expect("ERROR_NOT_FOUND_ORDER");

        // don hang da hoan thanh va chua refund
        assert!(order.is_completed && !order.is_refund);
        // let order = self.orders.find(order_id);

        order.is_refund = true;

        // cap nhat trang thai don va ghi de toan bo order moi lene order_id cu
        self.orders.insert(&order_id, &order);

        // Tra tien cho user
        // signer_account_id la vi goc cua admin
        if order.amount > 0 {
            // Cross contract call
            let promise: Promise = Promise::new(order.payer_id).transfer(order.amount).then(
                ext_self::ext(env::current_account_id())
                    .with_attached_deposit(0)
                    .with_static_gas(TRANSFER_GAS)
                    .transfer_callback(order_id),
            );
            PromiseOrValue::Promise(promise)
        } else {
            PromiseOrValue::Value(U128(0))
        }
    }
}

#[near_bindgen]
impl ExtEcommerceContract for EcommerceContract {
    #[private]
    fn transfer_callback(&mut self, order_id: OrderId) -> PromiseOrValue<U128> {
        assert_eq!(env::promise_results_count(), 1, "ERROR_TOO_MANY_RESULTS");

        match env::promise_result(0) {
            PromiseResult::NotReady => unreachable!(),
            PromiseResult::Successful(value) => PromiseOrValue::Value(U128(0)),
            PromiseResult::Failed => {
                // Cap nhat lai trang thai refund
                let mut order = self.orders.get(&order_id).expect("ERROR_ORDER_NOT_FOUND");
                order.is_refund = false;

                self.orders.insert(&order_id, &order);

                PromiseOrValue::Value(U128(order.amount))
            }
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod test {
    use std::task::Context;

    use super::*;
    use near_sdk::env::signer_account_id;
    use near_sdk::test_utils::{accounts, VMContextBuilder};
    use near_sdk::{testing_env, MockedBlockchain};

    fn get_context(is_view: bool) -> VMContextBuilder {
        let mut builder = VMContextBuilder::new();
        builder
            .current_account_id(accounts(0))
            .signer_account_id(accounts(0))
            .predecessor_account_id(accounts(0))
            .is_view(is_view);

        builder
    }

    #[test]
    fn test_pay_order() {
        let mut context = get_context(false);
        let alice: AccountId = accounts(0);

        context
            .account_balance(1000)
            .predecessor_account_id(alice.clone())
            .attached_deposit(1000)
            .signer_account_id(alice.clone());

        testing_env!(context.build());

        let mut contract = EcommerceContract::new(alice.clone());
        let order_amount = U128(1000);

        contract.pay_order("order_1".to_owned(), order_amount);

        let order = contract.get_order("order_1".to_owned());

        //Test
        assert_eq!(order.order_id, "order_1".to_owned());
        assert_eq!(order.amount, order_amount.0);
        assert_eq!(order.payer_id, alice);
        assert!(order.is_completed);
    }

    #[test]
    #[should_panic(expected = "ERROR_DEPOSIT_NOT_ENOUGH")]
    fn test_pay_order_with_lack_balance() {
        let mut context = get_context(false);
        let alice: AccountId = accounts(0);

        context
            .account_balance(1000)
            .predecessor_account_id(alice.clone())
            .attached_deposit(1000)
            .signer_account_id(alice.clone());

        testing_env!(context.build());

        let mut contract = EcommerceContract::new(alice.clone());
        let order_amount = U128(2000);

        contract.pay_order("order_1".to_owned(), order_amount);
    }
}
