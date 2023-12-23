#[macro_use]
extern crate serde;
use candid::{CandidType, Decode, Encode};
use ic_cdk::api::time;
use ic_stable_structures::memory_manager::{MemoryId, MemoryManager, VirtualMemory};
use ic_stable_structures::{BoundedStorable, Cell, DefaultMemoryImpl, StableBTreeMap, Storable};
use std::collections::HashMap;
use std::{borrow::Cow, cell::RefCell};

type Memory = VirtualMemory<DefaultMemoryImpl>;
type IdCell = Cell<u64, Memory>;

#[derive(CandidType, Clone, Serialize, Deserialize, PartialEq, Eq)]
enum MealType {
    Breakfast,
    Lunch,
    Dinner,
    Snack,
    Desserts,
}

#[derive(CandidType, Clone, Serialize, Deserialize)]
struct Ingredient {
    id: u64,
    name: String,
    quantity: u64,
    unit: String,
}

#[derive(CandidType, Clone, Serialize, Deserialize)]
struct Recipe {
    id: u64,
    name: String,
    ingredients: Vec<Ingredient>,
    instructions: Vec<String>,
    nutritional_info: Vec<String>,
    meal_type: MealType,
    created_at: u64,
    updated_at: Option<u64>,
}

// Implement traits for Recipe struct (Storable, BoundedStorable)
impl Storable for Recipe {
    fn to_bytes(&self) -> Cow<[u8]> {
        Cow::Owned(Encode!(self).unwrap())
    }

    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        Decode!(bytes.as_ref(), Self).unwrap()
    }
}

impl BoundedStorable for Recipe {
    const MAX_SIZE: u32 = 1024; // Define a suitable maximum size
    const IS_FIXED_SIZE: bool = false;
}

thread_local! {
    static MEMORY_MANAGER: RefCell<MemoryManager<DefaultMemoryImpl>> = RefCell::new(
        MemoryManager::init(DefaultMemoryImpl::default())
    );

    static ID_COUNTER: RefCell<IdCell> = RefCell::new(
        IdCell::init(MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(0))), 0)
            .expect("Cannot create a counter")
    );

    static RECIPE_STORAGE: RefCell<StableBTreeMap<u64, Recipe, Memory>> =
        RefCell::new(StableBTreeMap::init(
            MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(1)))
    ));

    // Include the inventory HashMap
     static INGREDIENT_INVENTORY: RefCell<HashMap<String, Ingredient>> = RefCell::new(HashMap::new());

    // Include the ingredient id counter
     static INGREDIENT_ID_COUNTER: RefCell<u64> = RefCell::new(0);
}

#[derive(CandidType, Serialize, Deserialize)]
struct RecipePayload {
    name: String,
    ingredients: Vec<Ingredient>,
    instructions: Vec<String>,
    nutritional_info: Vec<String>,
    meal_type: MealType,
}

#[derive(CandidType, Serialize, Deserialize)]
struct IngredientPayload {
    name: String,
    quantity: u64,
    unit: String,
}

#[ic_cdk::update]
fn add_recipe(recipe_payload: RecipePayload) -> Option<Recipe> {
    let id = ID_COUNTER
        .with(|counter| {
            let current_value = *counter.borrow().get();
            counter.borrow_mut().set(current_value + 1)
        })
        .expect("Cannot increment id counter");

    let ingredients = recipe_payload
        .ingredients
        .iter()
        .map(|ing| Ingredient {
            id: ing.id.clone(),
            name: ing.name.clone(),
            quantity: ing.quantity.clone(),
            unit: ing.unit.clone(),
        })
        .collect();

    let recipe = Recipe {
        id,
        name: recipe_payload.name,
        ingredients,
        instructions: recipe_payload.instructions,
        nutritional_info: recipe_payload.nutritional_info,
        meal_type: recipe_payload.meal_type,
        created_at: time(),
        updated_at: None,
    };

    do_insert_recipe(&recipe);
    Some(recipe)
}

#[ic_cdk::update]
fn update_recipe(id: u64, payload: RecipePayload) -> Result<Recipe, Error> {
    match RECIPE_STORAGE.with(|service| service.borrow().get(&id)) {
        Some(mut recipe) => {
            recipe.name = payload.name;
            recipe.ingredients = payload.ingredients;
            recipe.instructions = payload.instructions;
            recipe.nutritional_info = payload.nutritional_info;
            recipe.meal_type = payload.meal_type;
            recipe.updated_at = Some(time());
            do_insert_recipe(&recipe);
            Ok(recipe)
        }
        None => Err(Error::NotFound {
            msg: format!("Couldn't update a recipe with id={}. Recipe not found", id),
        }),
    }
}

#[ic_cdk::update]
fn delete_recipe(id: u64) -> Result<Recipe, Error> {
    match RECIPE_STORAGE.with(|service| service.borrow_mut().remove(&id)) {
        Some(recipe) => Ok(recipe),
        None => Err(Error::NotFound {
            msg: format!("Couldn't delete a recipe with id={}. Recipe not found", id),
        }),
    }
}

#[ic_cdk::query]
fn search_recipe(id: u64) -> Result<Recipe, Error> {
    match _get_recipe(&id) {
        Some(recipe) => Ok(recipe),
        None => Err(Error::NotFound {
            msg: format!("A recipe with id={} not found", id),
        }),
    }
}

#[ic_cdk::query]
fn search_recipe_by_meal_type(meal_type: MealType) -> Vec<Recipe> {
    let recipes = RECIPE_STORAGE.with(|service| {
        service
            .borrow()
            .iter()
            .map(|(_, recipe)| recipe.clone())
            .filter(|recipe| recipe.meal_type == meal_type)
            .collect::<Vec<Recipe>>()
    });

    recipes
}

// Function to view all Ingredients in inventory
#[ic_cdk::query]
fn view_inventory() -> Vec<Ingredient> {
    INGREDIENT_INVENTORY.with(|inventory| inventory.borrow().values().cloned().collect())
}

// Function to add ingredients to inventory
#[ic_cdk::update]
fn add_ingredient_to_inventory(ingredient: IngredientPayload) -> Option<Ingredient> {
    let new_ingredient_id = INGREDIENT_ID_COUNTER.with(|counter| {
        let current_id = *counter.borrow();
        *counter.borrow_mut() = current_id + 1; // Increment the counter
        current_id + 1 // Use the incremented value as the ID for the new ingredient
    });

    let new_ingredient = Ingredient {
        id: new_ingredient_id,
        name: ingredient.name.clone(),
        quantity: ingredient.quantity.clone(),
        unit: ingredient.unit.clone(),
    };

    INGREDIENT_INVENTORY.with(|inventory| {
        inventory
            .borrow_mut()
            .insert(ingredient.name.clone(), new_ingredient.clone());
    });

    Some(new_ingredient) // Return the newly added ingredient as an Option
}

// Function to remove ingredients from inventory
#[ic_cdk::update]
fn remove_ingredient_from_inventory(ingredient_name: String, quantity: u64) -> Result<Ingredient, Error> {
    let inventory = INGREDIENT_INVENTORY.try_with(|inv| inv.borrow().clone());

    match inventory {
        Ok(mut inventory) => {
            if let Some(ingredient) = inventory.get(&ingredient_name) {
                if ingredient.quantity == quantity {
                    if let Some(removed_ingredient) = inventory.remove(&ingredient_name) {
                        return Ok(removed_ingredient);
                    }
                }
            }
            Err(Error::NotFound {
                msg: format!("Ingredient '{}' with quantity '{}' not found in inventory", ingredient_name, quantity),
            })
        }
        Err(_) => Err(Error::NotFound {
            msg: "Failed to access inventory".to_string(), // Handle borrow failure
        }),
    }
}

#[ic_cdk::update]
fn generate_shopping_list(recipes: Vec<u64>) -> Vec<Ingredient> {
    let mut shopping_list = vec![];

    // Retrieve recipes by their IDs and check their ingredients against the inventory
    for recipe_id in recipes {
        if let Some(recipe) = _get_recipe(&recipe_id) {
            for ingredient in &recipe.ingredients {
                let is_available = INGREDIENT_INVENTORY.with(|inv| {
                    inv.borrow()
                        .get(&ingredient.name)
                        .map(|inv_item| inv_item.quantity.clone())
                });

                match is_available {
                    Some(quantity) if quantity == 0 => {
                        // If the quantity is zero or the ingredient is not found, add to shopping list
                        shopping_list.push(ingredient.clone());
                    }
                    None => {
                        // If the ingredient is not found in inventory, add it to the shopping list
                        shopping_list.push(ingredient.clone());
                    }
                    _ => (),
                }
            }
        }
    }

    shopping_list
}

#[derive(CandidType, Deserialize, Serialize)]
enum Error {
    NotFound { msg: String },
}

// Helper method to get a recipe by id. Used in get_recipe/update_recipe
fn _get_recipe(id: &u64) -> Option<Recipe> {
    RECIPE_STORAGE.with(|service| service.borrow().get(id))
}

// Helper method to perform recipe insert
fn do_insert_recipe(recipe: &Recipe) {
    RECIPE_STORAGE.with(|service| service.borrow_mut().insert(recipe.id, recipe.clone()));
}

// Candid interface generation
ic_cdk::export_candid!();
