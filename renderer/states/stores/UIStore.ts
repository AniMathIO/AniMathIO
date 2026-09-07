import { makeAutoObservable } from "mobx";
import type { RootStore } from "../RootStore";
import { MenuOption } from "@/types";

export class UIStore {
  selectedMenuOption: MenuOption = "Videos";

  constructor(private root: RootStore) {
    makeAutoObservable(this);
  }

  setSelectedMenuOption(option: MenuOption) {
    this.selectedMenuOption = option;
  }
}
