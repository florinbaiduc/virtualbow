#pragma once
#include "pre/widgets/PropertyList.hpp"

class SettingsModel;
class MainTreeModel;

class SettingsView: public PropertyList
{
public:
    SettingsView(SettingsModel* model, MainTreeModel* tree);
};
